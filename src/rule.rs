use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    time::UNIX_EPOCH,
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

const UNIX_EPOCH_ELAPSED_ERR: &str = "time travel is real";

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
            timestamp: UNIX_EPOCH
                .elapsed()
                .expect(UNIX_EPOCH_ELAPSED_ERR)
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
    subcommands("new", "amend", "repeal", "list", "history"),
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

/// Change the definition of a rule.
#[command(slash_command, required_permissions = "MANAGE_GUILD", ephemeral)]
async fn amend(
    ctx: poise_error::Context<'_>,
    #[description = "The rule to amend."]
    #[autocomplete = "rule_autocomplete"]
    #[rename = "rule"]
    rule_n: u64,
    #[description = "New definition of the rule."] text: String,
) -> anyhow::Result<()> {
    let guild_id = get_guild_id_from_ctx(ctx);
    let mut rules = Rules::get_data_from(guild_id)?;
    let rule = rules
        .get_mut(rule_n as usize - 1)
        .ok_or(UserError::from(format!("There is no rule {rule_n}.")))?;

    if rule.repealed.is_some() {
        bail!(UserError::from(format!(
            "Rule {rule_n} has been repealed, it cannot be amended now."
        )));
    }

    let prev_text = rule
        .amendments
        .last()
        .unwrap_or(&rule.original)
        .text
        .clone();

    rule.amendments.push(TimestampedText::new(&text));
    rules.set_data_for(guild_id)?;
    ctx.say(format!(
        "Rule {rule_n}, previously {prev_text:?}, has been amended to be {text:?}."
    ))
    .await?;
    update_rule_list(ctx, guild_id).await?;

    Ok(())
}

/// Declare a rule as no longer valid.
#[command(slash_command, required_permissions = "MANAGE_GUILD", ephemeral)]
async fn repeal(
    ctx: poise_error::Context<'_>,
    #[description = "The rule to repeal."]
    #[autocomplete = "rule_autocomplete"]
    #[rename = "rule"]
    rule_n: u64,
) -> anyhow::Result<()> {
    let guild_id = get_guild_id_from_ctx(ctx);
    let mut rules = Rules::get_data_from(guild_id)?;
    let rule = rules
        .get_mut(rule_n as usize - 1)
        .ok_or(UserError::from(format!("There is no rule {rule_n}.")))?;

    if rule.repealed.is_some() {
        bail!(UserError::from(format!(
            "Rule {rule_n} has already been repealed."
        )));
    }

    rule.repealed = Some(
        UNIX_EPOCH
            .elapsed()
            .expect(UNIX_EPOCH_ELAPSED_ERR)
            .as_secs(),
    );

    let rule_text = rule
        .amendments
        .last()
        .unwrap_or(&rule.original)
        .text
        .clone();

    rules.set_data_for(guild_id)?;
    ctx.say(format!("Rule {rule_n}, {rule_text:?}, has been repealed."))
        .await?;
    update_rule_list(ctx, guild_id).await?;

    Ok(())
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
                let rule_n = i as u64 + 1;

                (
                    rule_n,
                    format!(
                        "{rule_n}. {}",
                        rule.amendments.last().unwrap_or(&rule.original).text,
                    ),
                )
            })
            .filter(|(_rule_n, str)| str.to_lowercase().contains(query))
            .map(|(rule_n, str)| AutocompleteChoice::new(str, rule_n))
            .collect::<Vec<AutocompleteChoice>>(),
    )
}

async fn update_rule_list(ctx: poise_error::Context<'_>, guild_id: GuildId) -> anyhow::Result<()> {
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
                    UNIX_EPOCH
                        .elapsed()
                        .expect(UNIX_EPOCH_ELAPSED_ERR)
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

/// Lists everything that's happened to the rules in chronological order.
#[command(slash_command, ephemeral)]
async fn history(ctx: poise_error::Context<'_>) -> anyhow::Result<()> {
    enum Event {
        Instated(String),
        Amended(String),
        Repealed,
    }

    impl std::fmt::Display for Event {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Event::Instated(text) => write!(f, "instated as {text:?}"),
                Event::Amended(text) => write!(f, "amended to {text:?}"),
                Event::Repealed => write!(f, "repealed"),
            }
        }
    }

    let rules = Rules::get_data_from(get_guild_id_from_ctx(ctx))?.0;
    let mut events: BTreeMap<u64, (usize, Event)> = BTreeMap::new();

    for (i, rule) in rules.into_iter().enumerate() {
        events.insert(
            rule.original.timestamp,
            (i, Event::Instated(rule.original.text)),
        );

        for amendment in rule.amendments {
            events.insert(amendment.timestamp, (i, Event::Amended(amendment.text)));
        }

        if let Some(repealed) = rule.repealed {
            events.insert(repealed, (i, Event::Repealed));
        }
    }

    let mut events_list = String::new();

    for (i, (timestamp, (rule_i, event))) in events.into_iter().enumerate() {
        if i != 0 {
            events_list += "\n";
        }

        events_list += &format!("<t:{timestamp}:f>: rule {} was {event}.", rule_i + 1);
    }

    ctx.send(
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(vec![CreateComponent::Container(CreateContainer::new(
                vec![
                    CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                        "### Rule History\n\
                         {}",
                        if events_list.is_empty() {
                            "*Intentionally left blank.*".to_string()
                        } else {
                            events_list
                        }
                    ))),
                    CreateComponent::Separator(CreateSeparator::new(true)),
                    CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                        "-# Last updated <t:{}:R>",
                        UNIX_EPOCH
                            .elapsed()
                            .expect(UNIX_EPOCH_ELAPSED_ERR)
                            .as_secs(),
                    ))),
                ],
            ))]),
    )
    .await?;

    Ok(())
}
