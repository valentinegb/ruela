mod data;

use std::time::{SystemTime, UNIX_EPOCH};

use poise::{
    CreateReply, command,
    samples::create_application_commands,
    serenity_prelude::{
        self as serenity, CreateCommand, CreateComponent, CreateContainer, CreateSeparator,
        CreateTextDisplay, FullEvent, GatewayIntents, MessageFlags, Permissions, StatusCode, Token,
        async_trait,
    },
};
use poise_error::{
    UserError,
    anyhow::{self, bail},
};
use tracing::{error, info, warn};

use crate::data::{Data, INVOCABLE_IN_GUILD, Rule, Rules, RulesMessage, get_guild_id_from_ctx};

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

/// Commands related to the rules of this server.
#[command(
    slash_command,
    subcommands("new", "list"),
    interaction_context = "Guild"
)]
async fn rule(_ctx: poise_error::Context<'_>) -> anyhow::Result<()> {
    unreachable!()
}

/// Create a new rule.
#[command(slash_command, required_permissions = "MANAGE_GUILD")]
async fn new(
    ctx: poise_error::Context<'_>,
    #[description = "Content of the rule."] text: String,
) -> anyhow::Result<()> {
    todo!("check if the user has permission to create rules");

    let guild_id = get_guild_id_from_ctx(ctx);
    let mut rules = Rules::get_data_from(guild_id)?;

    rules.push(Rule::new(text));
    rules.set_data_for(guild_id)?;

    todo!("give feedback to the user");

    Ok(())
}

/// Lists the rules of this server.
#[command(slash_command)]
async fn list(
    ctx: poise_error::Context<'_>,
    #[description = "Whether the list should be visible to all and update automatically."]
    persistent: Option<bool>,
) -> anyhow::Result<()> {
    let persistent = persistent.is_some_and(|persistent| persistent);
    let guild_id = get_guild_id_from_ctx(ctx);

    if persistent {
        if !ctx
            .author_member()
            .await
            .expect(INVOCABLE_IN_GUILD)
            .permissions
            .expect("should be in the context of an interaction")
            .manage_guild()
        {
            bail!(UserError::from(format!(
                "You must have permission to {} to create a persistent rules list.",
                Permissions::MANAGE_GUILD,
            )));
        }

        if let Some(result) = RulesMessage::get_data_from(guild_id)?.get(ctx).await
            && !result.as_ref().is_err_and(|err| {
                if let serenity::Error::Http(http_err) = err
                    && http_err
                        .status_code()
                        .is_some_and(|status_code| status_code == StatusCode::NOT_FOUND)
                {
                    true
                } else {
                    false
                }
            })
        {
            bail!(UserError::from(format!(
                "A persistent rule list already exists and there can only be one \
                 per server.\n\
                 Please delete {} if you want to send a new one.",
                result?.link(),
            )));
        }

        ctx.defer().await?;
    } else {
        ctx.defer_ephemeral().await?;
    }

    let rules = Rules::get_data_from(guild_id)?;
    let message = ctx
        .send(
            CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(&[CreateComponent::Container(CreateContainer::new(&[
                    CreateComponent::TextDisplay(CreateTextDisplay::new(
                        "### Rules\n\
                     1\\. Placeholder rule\n\
                     2\\. Placeholder rule\n\
                     4\\. Placeholder rule\n\
                     5\\. Placeholder rule",
                    )),
                    CreateComponent::Separator(CreateSeparator::new(true)),
                    CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                        "-# Last updated <t:{}:R>",
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .expect("time travel is real")
                            .as_secs(),
                    ))),
                ]))]),
        )
        .await?;

    if persistent {
        RulesMessage::from(message.into_message().await?).set_data_for(guild_id)?;
    }

    Ok(())
}
