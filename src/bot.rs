use crate::gh::Gh;

use std::sync::Arc;

use discord::{
    async_trait,
    model::{channel::Message, id::ChannelId},
    prelude::*,
};

use parking_lot::Mutex as PMutex;
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use url::Url;

const DATA_FILENAME: &str = "data.toml";
const PR_PATH_PREFIX: &str = "/NixOS/nixpkgs/pull/";

#[derive(Debug, Default, Deserialize, Serialize)]
struct BotData {
    pr_channel: Option<ChannelId>,
}

#[derive(Debug, Clone)]
pub struct Bot<'a> {
    gh: Gh<'a>,
    data: Arc<PMutex<BotData>>,
}

impl<'a> Bot<'a> {
    pub fn new(gh: Gh<'a>) -> Self {
        Self {
            gh,
            data: Default::default(),
        }
    }

    pub async fn write_data(&self) {
        let serialized = toml::to_vec(&*self.data.lock()).expect("failed to ser data");
        tokio::fs::write(DATA_FILENAME, serialized)
            .await
            .expect("failed to write data");
    }

    pub async fn read_data(&self) {
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
        match self.gh.pulls().get(pr_number).await {
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
impl<'a> EventHandler for Bot<'a> {
    async fn message(&self, ctx: Context, message: Message) {
        let is_pr_channel = self
            .data
            .lock()
            .pr_channel
            .as_ref()
            .map_or(false, |id| message.channel_id.eq(id));

        if let (true, Ok(pr_url)) = (is_pr_channel, message.content.parse::<Url>()) {
            let maybe_pr_number = pr_url
                .path()
                .strip_prefix(PR_PATH_PREFIX)
                .map(|maybe_num| maybe_num.parse::<u64>().ok())
                .flatten();
            if let Some(pr_number) = maybe_pr_number {
                self.pr_handler(&ctx, &message, pr_number).await;
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
