pub mod bot;
pub mod gh;

use bot::Bot;

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::gh::Gh;

const LOG_FILENAME_PREFIX: &str = "log";

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
    let inner_gh = github::initialise(builder).expect("failed to initialize github client");

    // Build our bot and read data
    let bot = Bot::new(Gh::new(inner_gh));
    bot.read_data().await;

    // Build discord client
    let mut client = discord::Client::builder(&discord_token)
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
