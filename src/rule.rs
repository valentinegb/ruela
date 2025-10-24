use std::{
    ops::{Deref, DerefMut},
    time::{SystemTime, UNIX_EPOCH},
};

use poise::{
    CreateReply, command,
    serenity_prelude::{
        self as serenity, AutocompleteChoice, CacheHttp, CreateAutocompleteResponse,
        CreateComponent, CreateContainer, CreateSeparator, CreateTextDisplay, EditMessage,
        GenericChannelId, GuildId, Message, MessageFlags, MessageId, Permissions, StatusCode,
    },
};
use poise_error::{
    UserError,
    anyhow::{self, bail},
};
use serde::{Deserialize, Serialize};

use crate::data::{Data, INVOCABLE_IN_GUILD, get_guild_id_from_ctx};

#[derive(Deserialize, Serialize, Default)]
struct Rules(Vec<Rule>);

impl Data<GuildId> for Rules {
    const PATH: &str = "rules.cbor";
    const DESCRIPTOR: &str = "server rules";
}

impl Deref for Rules {
    type Target = Vec<Rule>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Rules {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Deserialize, Serialize)]
struct Rule {
    original: TimestampedText,
    amendments: Vec<TimestampedText>,
    repealed: Option<u64>,
}

impl Rule {
    fn new(text: impl Into<String>) -> Self {
        Self {
            original: TimestampedText::new(text),
            amendments: Vec::new(),
            repealed: None,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct TimestampedText {
    text: String,
    timestamp: u64,
}

impl TimestampedText {
    fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time travel is real")
                .as_secs(),
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct RulesMessage(Option<(GenericChannelId, MessageId)>);

impl Data<GuildId> for RulesMessage {
    const PATH: &str = "rules_message.cbor";
    const DESCRIPTOR: &str = "persistent rule list message";
}

impl From<Message> for RulesMessage {
    fn from(value: Message) -> Self {
        Self(Some((value.channel_id, value.id)))
    }
}

impl RulesMessage {
    async fn get(&self, cache_http: impl CacheHttp) -> Option<Result<Message, serenity::Error>> {
        match self.0 {
            Some((channel_id, message_id)) => {
                Some(channel_id.message(cache_http, message_id).await)
            }
            None => None,
        }
    }
}

/// Commands related to the rules of this server.
#[command(
    slash_command,
    subcommands("new", "repeal", "list"),
    interaction_context = "Guild"
)]
pub async fn rule(_ctx: poise_error::Context<'_>) -> anyhow::Result<()> {
    unreachable!()
}

/// Create a new rule.
#[command(slash_command, required_permissions = "MANAGE_GUILD", ephemeral)]
async fn new(
    ctx: poise_error::Context<'_>,
    #[description = "Content of the rule."] text: String,
) -> anyhow::Result<()> {
    let guild_id = get_guild_id_from_ctx(ctx);
    let mut rules = Rules::get_data_from(guild_id)?;

    rules.push(Rule::new(&text));
    rules.set_data_for(guild_id)?;
    ctx.say(format!(
        "Rule {}, {text:?}, has been instated.",
        rules.len(),
    ))
    .await?;

    if let Some(result) = RulesMessage::get_data_from(guild_id)?.get(ctx).await
        && !result.as_ref().is_err_and(is_not_found_error)
    {
        let mut message = result?;

        message
            .edit(
                ctx,
                compile_rule_list(guild_id)?.to_prefix_edit(EditMessage::new()),
            )
            .await?;
    }

    Ok(())
}

/// Declare a rule as no longer valid.
#[command(slash_command, required_permissions = "MANAGE_GUILD", ephemeral)]
async fn repeal(
    ctx: poise_error::Context<'_>,
    #[description = "The rule to repeal."]
    #[autocomplete = "rule_autocomplete"]
    rule: usize,
) -> anyhow::Result<()> {
    let mut rules = Rules::get_data_from(get_guild_id_from_ctx(ctx))?;

    todo!()
}

async fn rule_autocomplete<'a>(
    ctx: poise_error::Context<'_>,
    query: &str,
) -> CreateAutocompleteResponse<'a> {
    // Noooo! You can't just read and deserialize the rules every single time a
    // user types a letter into the rule option!!!!
    // The chad:
    let rules = Rules::get_data_from(get_guild_id_from_ctx(ctx)).unwrap();
    // It's better for this ^ to panic than for this function to return an empty
    // list, because returning an empty list will cause Discord to say there
    // were no results, while panicking will cause Discord to say that an error
    // occurred, which is more correct.

    CreateAutocompleteResponse::new().set_choices(
        rules
            .iter()
            .enumerate()
            .filter(|(_i, rule)| rule.repealed.is_none())
            .map(|(i, rule)| {
                (
                    i,
                    format!(
                        "{}. {}",
                        i + 1,
                        rule.amendments.last().unwrap_or(&rule.original).text,
                    ),
                )
            })
            .filter(|(_i, str)| str.to_lowercase().contains(query))
            .map(|(i, str)| {
                AutocompleteChoice::new(
                    str,
                    u64::try_from(i)
                        .expect("should be running on a system that isn't more than 64 bit"),
                )
            })
            .collect::<Vec<AutocompleteChoice>>(),
    )
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
                "You must have permission to {} to create a persistent rule list.",
                Permissions::MANAGE_GUILD,
            )));
        }

        if let Some(result) = RulesMessage::get_data_from(guild_id)?.get(ctx).await
            && !result.as_ref().is_err_and(is_not_found_error)
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

    let message = ctx.send(compile_rule_list(guild_id)?).await?;

    if persistent {
        RulesMessage::from(message.into_message().await?).set_data_for(guild_id)?;
    }

    Ok(())
}

fn compile_rule_list<'a>(guild_id: GuildId) -> anyhow::Result<CreateReply<'a>> {
    let rules = Rules::get_data_from(guild_id)?;
    let rule_list_items: Vec<String> = rules
        .iter()
        .enumerate()
        .filter(|(_i, rule)| rule.repealed.is_none())
        .map(|(i, rule)| {
            format!(
                "{}\\. {}",
                i + 1,
                rule.amendments.last().unwrap_or(&rule.original).text,
            )
        })
        .collect();

    Ok(CreateReply::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![CreateComponent::Container(CreateContainer::new(
            vec![
                CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "### Rules\n\
                 {}",
                    if rule_list_items.is_empty() {
                        "There are none. Let there be anarchy!".to_string()
                    } else {
                        rule_list_items.join("\n")
                    }
                ))),
                CreateComponent::Separator(CreateSeparator::new(true)),
                CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "-# Last updated <t:{}:R>",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("time travel is real")
                        .as_secs(),
                ))),
            ],
        ))]))
}

fn is_not_found_error(err: &serenity::Error) -> bool {
    if let serenity::Error::Http(http_err) = err
        && http_err
            .status_code()
            .is_some_and(|status_code| status_code == StatusCode::NOT_FOUND)
    {
        true
    } else {
        false
    }
}
