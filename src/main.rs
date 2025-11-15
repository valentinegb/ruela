mod data;
mod rule;
mod strikes;
mod util;

use poise::{
    samples::create_application_commands,
    serenity_prelude::{
        self as serenity, CreateCommand, FullEvent, GatewayIntents, Token, async_trait,
    },
};
use poise_error::anyhow;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::{rule::rule, strikes::strike};

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
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("ruela=debug,info"))
        .unwrap();
    let registry = tracing_subscriber::registry().with(filter_layer);

    match tracing_journald::layer() {
        Ok(journald_layer) => {
            registry.with(journald_layer).init();
        }
        Err(_) => {
            let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);

            registry.with(fmt_layer).init();
        }
    }

    if let Err(err) = try_main().await {
        error!("Fatal error: {err:#}");
    }
}

async fn try_main() -> anyhow::Result<()> {
    info!("Starting up");

    #[cfg(debug_assertions)]
    if let Err(err) = dotenvy::dotenv() {
        tracing::warn!("Could not load `.env` file: {err:#}");
    }

    let commands = vec![rule(), strike()];
    let mut client = serenity::Client::builder(
        Token::from_env("RUELA_DISCORD_TOKEN")?,
        GatewayIntents::GUILDS,
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
