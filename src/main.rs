mod data;
mod rule;

use poise::{
    samples::create_application_commands,
    serenity_prelude::{
        self as serenity, CreateCommand, FullEvent, GatewayIntents, Token, async_trait,
    },
};
use poise_error::anyhow;
use tracing::{error, info, warn};

use crate::rule::rule;

struct EventHandler<'a> {
    commands: Vec<CreateCommand<'a>>,
}

#[async_trait]
impl serenity::EventHandler for EventHandler<'_> {
    async fn dispatch(&self, ctx: &serenity::Context, event: &FullEvent) {
        if let FullEvent::Ready { data_about_bot, .. } = event {
            info!("Logged in as {}", data_about_bot.user.tag());

            if let Err(err) =
                serenity::Command::set_global_commands(&ctx.http, &self.commands).await
            {
                error!("Could not register commands: {err:#}");
            }

            info!("Registered commands");
        }
    }
}

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

    let commands = vec![rule()];
    let mut client = serenity::Client::builder(
        Token::from_env("MOD_BOT_DISCORD_TOKEN")?,
        GatewayIntents::empty(),
    )
    .event_handler(EventHandler {
        commands: create_application_commands(&commands),
    })
    .framework(poise::Framework::new(poise::FrameworkOptions {
        commands,
        on_error: poise_error::on_error,
        pre_command: |ctx: poise_error::Context| {
            Box::pin(async move {
                info!("{} invoked {}", ctx.author().tag(), ctx.invocation_string());
            })
        },
        ..Default::default()
    }))
    .await?;

    client.start_autosharded().await?;
    info!("Shutting down");

    Ok(())
}
