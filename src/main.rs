use std::sync::Arc;

use discord::{
    async_trait,
    model::{channel::Message, id::ChannelId},
    prelude::*,
};

use github::{models::pulls::PullRequest, Result as GhResult};
use parking_lot::Mutex as PMutex;
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use url::Url;

type Gh = Arc<github::Octocrab>;

const LOG_FILENAME_PREFIX: &str = "log";
const DATA_FILENAME: &str = "data.toml";
const PR_PATH_PREFIX: &str = "/NixOS/nixpkgs/pull/";

#[derive(Debug, Default, Deserialize, Serialize)]
struct BotData {
    pr_channel: Option<ChannelId>,
}

#[derive(Debug, Clone)]
struct Bot {
    gh: Gh,
    data: Arc<PMutex<BotData>>,
}

impl Bot {
    async fn write_data(&self) {
        let serialized = toml::to_vec(&*self.data.lock()).expect("failed to ser data");
        tokio::fs::write(DATA_FILENAME, serialized)
            .await
            .expect("failed to write data");
    }

    async fn read_data(&self) {
        let raw_data = tokio::fs::read(DATA_FILENAME).await;
        let data = raw_data
            .map(|data| toml::from_slice(&data).expect("failed to deser data"))
            .unwrap_or_default();
        *self.data.lock() = data;
    }

    async fn pr_handler(&self, ctx: &Context, msg: &Message, pr_number: u64) {
        info!(
            "PR link received with PR number {} from user {}",
            pr_number, msg.author.id
        );
        match get_pr(&self.gh, pr_number).await {
            Ok(pr) => {
                let content = format!("<@{}>: {} | <{}>", msg.author.id, pr.title, msg.content,);
                let result = msg
                    .channel_id
                    .send_message(&ctx, |m| m.content(content))
                    .await;
                if let Err(err) = result {
                    error!(
                        "Failed to send message in channel {}: {}",
                        msg.channel_id, err
                    );
                } else if let Err(err) = msg.delete(&ctx).await {
                    error!(
                        "Failed to delete message {} in channel {}: {}",
                        msg.id, msg.channel_id, err
                    );
                }
            }
            Err(err) => {
                error!("Failed to get PR: {}", err);
                reply_with(&ctx, &msg, format!("No such PR? ({})", err)).await;
            }
        }
    }

    async fn set_handler(&self, ctx: &Context, msg: &Message, subcmd: &str) {
        let reply_with = |content| reply_with(ctx, msg, content);
        let has_perm = if let Some(guild) = msg.guild(&ctx).await {
            let member = guild.member(&ctx, msg.author.id).await.unwrap();
            member.permissions(&ctx).await.unwrap().manage_guild()
        } else {
            return;
        };
        if has_perm {
            match subcmd {
                "prchannel" => {
                    self.data.lock().pr_channel = Some(msg.channel_id);
                    info!("Set pr_channel to {}", msg.channel_id);
                    reply_with("Listening for `nixpkgs` PR URLs in this channel!").await;
                }
                _ => {
                    reply_with("No such command.").await;
                }
            }
        } else {
            info!(
                "User {} does not have enough permissions to set options.",
                msg.author.id
            );
            reply_with("You don't have enough permissions to manage the bot.").await;
        }
    }
}

#[async_trait]
impl EventHandler for Bot {
    async fn message(&self, ctx: Context, message: Message) {
        let chan_id = message.channel_id;
        let pr_channel = self.data.lock().pr_channel;

        if let Some(pr_chan_id) = pr_channel {
            if chan_id == pr_chan_id {
                if let Ok(pr_url) = message.content.parse::<Url>() {
                    let maybe_pr_number = pr_url
                        .path()
                        .strip_prefix(PR_PATH_PREFIX)
                        .map(|maybe_num| maybe_num.parse::<u64>().ok())
                        .flatten();
                    if let Some(pr_number) = maybe_pr_number {
                        self.pr_handler(&ctx, &message, pr_number).await;
                    }
                }
            }
        }

        if let Some(subcmd) = message.content.strip_prefix("$set ") {
            self.set_handler(&ctx, &message, subcmd).await;
        }
    }
}

async fn reply_with(ctx: &Context, msg: &Message, content: impl std::fmt::Display) {
    if let Err(err) = msg.reply(&ctx, content).await {
        error!(
            "failed to send message in channel {}: {}",
            msg.channel_id, err
        );
    };
}

#[tokio::main]
async fn main() {
    use std::env::var;

    // Get tokens
    let gh_token = var("GH_TOKEN");
    let discord_token = var("DISCORD_TOKEN").expect("i need a discord bot token to work");

    // Set up logging
    let term_logger = fmt::layer();
    let file_appender = tracing_appender::rolling::daily("logs", LOG_FILENAME_PREFIX);
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let file_logger = fmt::layer().with_ansi(false).with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::from("info")))
        .with(term_logger)
        .with(file_logger)
        .init();

    // Build github client
    let mut builder = github::OctocrabBuilder::default();
    if let Ok(token) = gh_token {
        builder = builder.personal_token(token);
    }
    let gh = github::initialise(builder).expect("failed to initialize github client");

    // Build our bot and read data
    let bot = Bot {
        gh,
        data: Default::default(),
    };
    bot.read_data().await;

    // Build discord client
    let mut client = Client::builder(&discord_token)
        .event_handler(bot.clone())
        .await
        .expect("failed to create discord client");

    // Handle ctrl-c
    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("failed to ctrl c");
        bot.write_data().await;
        shard_manager.lock().await.shutdown_all().await;
    });

    // Start bot
    client
        .start()
        .await
        .expect("error occured in discord client");
}

async fn get_pr(gh: &Gh, number: u64) -> GhResult<PullRequest> {
    gh.pulls("NixOS", "nixpkgs").get(number).await
}
