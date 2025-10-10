use poise::serenity_prelude::{self as serenity, GatewayIntents, Token};
use poise_error::anyhow;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    if let Err(err) = try_main().await {
        error!("Fatal error: {err:#}");
    }
}

async fn try_main() -> anyhow::Result<()> {
    info!("Starting up");

    #[cfg(debug_assertions)]
    if let Err(err) = dotenvy::dotenv() {
        warn!("Could not load `.env` file: {err:#}");
    }

    let mut client = serenity::Client::builder(
        Token::from_env("MOD_BOT_DISCORD_TOKEN")?,
        GatewayIntents::empty(),
    )
    .await?;

    client.start_autosharded().await?;
    info!("Shutting down");

    Ok(())
}
