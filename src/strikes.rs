use std::{
    ops::{Deref, DerefMut},
    str::FromStr,
    time::UNIX_EPOCH,
};

use crate::{
    data::{Attribution, Data, INVOCABLE_IN_GUILD, UNIX_EPOCH_ELAPSED_ERR},
    rule::{Rules, rule_autocomplete},
    util::{ConfirmationPrompt, send_safety_alert},
};
use poise::{
    CreateReply, command,
    serenity_prelude::{
        self as serenity, CreateAllowedMentions, CreateComponent, CreateContainer, CreateMessage,
        CreateSeparator, CreateTextDisplay, GuildId, Member, Mentionable, MessageFlags,
        colours::css::{DANGER, POSITIVE},
    },
};
use poise_error::{
    UserError,
    anyhow::{self, bail},
};
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Deserialize, Serialize, Default)]
struct Strikes(Vec<Strike>);

impl Data<Member> for Strikes {
    const PATH: &str = "strikes.cbor";
    const DESCRIPTOR: &str = "member strikes";
}

impl Deref for Strikes {
    type Target = Vec<Strike>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Strikes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Deserialize, Serialize, Clone)]
struct Strike {
    rule_i: Option<usize>,
    notes: Option<String>,
    attribution: Attribution,
    repeal: Option<Attribution>,
}

#[derive(Deserialize, Serialize, Default)]
struct BanThreshold(Option<usize>);

impl Data<GuildId> for BanThreshold {
    const PATH: &str = "ban_threshold.cbor";
    const DESCRIPTOR: &str = "ban threshold";
}

impl Deref for BanThreshold {
    type Target = Option<usize>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for BanThreshold {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Commands related to strikes.
#[command(
    slash_command,
    subcommands("issue", "info", "repeal", "record", "ban_threshold"),
    interaction_context = "Guild"
)]
pub async fn strike(_ctx: poise_error::Context<'_>) -> anyhow::Result<()> {
    unreachable!("/strike has subcommands and itself should not be invocable")
}

/// Issue a strike to someone.
#[command(slash_command, required_permissions = "MODERATE_MEMBERS", ephemeral)]
async fn issue(
    ctx: poise_error::Context<'_>,
    #[description = "The person to issue the strike to."] member: Member,
    #[description = "The rule the person violated."]
    #[autocomplete = "rule_autocomplete"]
    #[rename = "rule"]
    rule_n: Option<u64>,
    #[description = "Any extra information that may be useful to include for the record."]
    notes: Option<String>,
) -> anyhow::Result<()> {
    let strike = Strike {
        // `rule_i` starts from 0, `rule_n` starts from 1.
        rule_i: rule_n.map(|rule_n| (rule_n - 1) as usize),
        notes,
        attribution: ctx.author().id.into(),
        repeal: None,
    };
    let mut strikes = Strikes::get_data_from(&member)?;
    let components = vec![component_for_strike(&member, strikes.len() + 1, &strike)?];
    let allowed_mentions = CreateAllowedMentions::new();
    let prompt = ConfirmationPrompt {
        components: components.clone(),
        prompt: Some("Are you sure you want issue this strike?"),
        elaboration: Some("The member will be notified and you will be attributed."),
        confirm_text: Some("Issue it"),
        confirmed_text: Some("Strike issued"),
        allowed_mentions: Some(allowed_mentions.clone()),
        ..Default::default()
    };
    let confirmed = prompt.prompt(ctx).await?;

    if confirmed {
        strikes.push(strike.clone());
        strikes.set_data_for(&member)?;

        let message = CreateMessage::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components)
            .allowed_mentions(allowed_mentions);
        let dms = member.user.create_dm_channel(ctx).await?.id.widen();

        dms.send_message(ctx.http(), message.clone()).await?;
        send_safety_alert(ctx, message).await?;

        if let Some(ban_threshold) =
            *BanThreshold::get_data_from(ctx.guild_id().expect(INVOCABLE_IN_GUILD))?
        {
            let non_repealed_strikes = strikes
                .iter()
                .filter(|strike| strike.repeal.is_none())
                .count();

            if non_repealed_strikes >= ban_threshold {
                if let Err(err) = dms.send_message(
                    ctx.http(),
                    CreateMessage::new()
                        .flags(MessageFlags::IS_COMPONENTS_V2)
                        .components(&[CreateComponent::Container(
                            CreateContainer::new(&[CreateComponent::TextDisplay(
                                CreateTextDisplay::new(format!(
                                    "### Strike Ban Threshold Met\n\
                                    You have accumulated **{non_repealed_strikes} strikes** and thus are being automatically banned.",
                                )),
                            )])
                            .accent_color(DANGER),
                        )]),
                )
                .await {
                    error!("Failed to inform member about being banned: {err:#}");
                }

                member
                    .ban(ctx.http(), 0, Some("Strike ban threshold met."))
                    .await?;
                send_safety_alert(
                    ctx,
                    CreateMessage::new()
                        .flags(MessageFlags::IS_COMPONENTS_V2)
                        .components(&[CreateComponent::Container(
                            CreateContainer::new(&[CreateComponent::TextDisplay(
                                CreateTextDisplay::new(format!(
                                    "### Strike Ban Threshold Met\n\
                                    {} accumulated **{non_repealed_strikes} strikes** and thus is being automatically banned.",
                                    member.mention(),
                                )),
                            )])
                            .accent_color(DANGER),
                        )])
                        .allowed_mentions(CreateAllowedMentions::new()),
                )
                .await?;
            }
        }
    }

    Ok(())
}

/// See info on a strike issued to someone.
#[command(slash_command, ephemeral)]
async fn info(
    ctx: poise_error::Context<'_>,
    #[description = "The strike to see info on."]
    #[rename = "strike"]
    strike_n: usize,
    #[description = "The person the strike is on. Yourself, if not specified."] member: Option<
        Member,
    >,
) -> anyhow::Result<()> {
    let author_member = ctx.author_member().await.expect(INVOCABLE_IN_GUILD);
    let member = member.as_ref().unwrap_or(author_member.as_ref());

    ensure_author_has_perms(ctx, member).await?;

    let strikes = Strikes::get_data_from(member)?;
    let strike = strikes.get(strike_n - 1).ok_or(UserError::from(format!(
        "There is no strike {strike_n} for {}.",
        member.mention(),
    )))?;

    ctx.send(
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(&[component_for_strike(member, strike_n, strike)?])
            .allowed_mentions(CreateAllowedMentions::new()),
    )
    .await?;

    Ok(())
}

/// Declare a strike as no longer valid.
#[command(slash_command, required_permissions = "MODERATE_MEMBERS", ephemeral)]
async fn repeal(
    ctx: poise_error::Context<'_>,
    #[description = "The strike to repeal."]
    #[rename = "strike"]
    strike_n: usize,
    #[description = "The person the strike is on."] member: Member,
) -> anyhow::Result<()> {
    if &member.user == ctx.author() {
        bail!(UserError::from_str("You can't repeal a strike issued to yourself.").unwrap());
    }

    let mut strikes = Strikes::get_data_from(&member)?;
    let strike = strikes
        .get_mut(strike_n - 1)
        .ok_or(UserError::from(format!(
            "There is no strike {strike_n} for {}.",
            member.mention(),
        )))?;

    strike.repeal = Some(ctx.author().id.into());

    let owned_strike = strike.clone();
    let components = vec![component_for_strike(&member, strike_n, &owned_strike)?];
    let allowed_mentions = CreateAllowedMentions::new();
    let prompt = ConfirmationPrompt {
        components: components.clone(),
        prompt: Some("Are you sure you want repeal this strike?"),
        elaboration: Some("The member will be notified and you will be attributed."),
        confirm_text: Some("Repeal it"),
        confirmed_text: Some("Strike repealed"),
        allowed_mentions: Some(allowed_mentions.clone()),
        ..Default::default()
    };
    let confirmed = prompt.prompt(ctx).await?;

    if confirmed {
        strikes.set_data_for(&member)?;

        let message = CreateMessage::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components)
            .allowed_mentions(allowed_mentions);

        member
            .user
            .create_dm_channel(ctx)
            .await?
            .id
            .widen()
            .send_message(ctx.http(), message.clone())
            .await?;
        send_safety_alert(ctx, message).await?;
    }

    Ok(())
}

/// See the all the strikes a person has a received.
#[command(slash_command, ephemeral)]
async fn record(
    ctx: poise_error::Context<'_>,
    #[description = "The person to see the record of. Yourself, if not specified."] member: Option<
        Member,
    >,
) -> anyhow::Result<()> {
    let author_member = ctx.author_member().await.expect(INVOCABLE_IN_GUILD);
    let member = member.as_ref().unwrap_or(author_member.as_ref());

    ensure_author_has_perms(ctx, member).await?;

    let strikes = Strikes::get_data_from(member)?;
    let mut list = String::new();
    let mut color = DANGER;

    for (i, strike) in strikes.iter().enumerate() {
        let strikethrough = if strike.repeal.is_some() { "~~" } else { "" };
        let rule_text = if let Some(rule_i) = strike.rule_i {
            &format!("rule {}", rule_i + 1)
        } else {
            "*no rule specified*"
        };

        list += &format!(
            "\n{strikethrough}**Strike {}**: {rule_text}{strikethrough}",
            i + 1
        );
    }

    let list_or_all_clean = if strikes.is_empty() {
        color = POSITIVE;

        "\n*All clean!*".to_string()
    } else {
        list
    };

    let strike_info_command_mention = if let Some(command) = ctx
        .http()
        .get_global_commands()
        .await?
        .into_iter()
        .find(|command| command.name == "strike")
    {
        &format!("</strike info:{}>", command.id)
    } else {
        "/strike info"
    };
    let mut container_components = vec![CreateComponent::TextDisplay(CreateTextDisplay::new(
        format!(
            "### {}'s Strike Record{list_or_all_clean}",
            member.mention()
        ),
    ))];

    if !strikes.is_empty() {
        container_components.extend_from_slice(&[
            CreateComponent::Separator(CreateSeparator::new(true)),
            CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                "-# Use {strike_info_command_mention} to see more about a particular strike."
            ))),
        ]);
    }

    ctx.send(
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(&[CreateComponent::Container(
                CreateContainer::new(container_components).accent_color(color),
            )])
            .allowed_mentions(CreateAllowedMentions::new()),
    )
    .await?;

    Ok(())
}

/// If a person's number of strikes meet the the threshold, they will be auto-banned.
#[command(slash_command, subcommands("get", "set"))]
async fn ban_threshold(_ctx: poise_error::Context<'_>) -> anyhow::Result<()> {
    unreachable!("/strike ban_threshold has subcommands and itself should not be invocable")
}

/// Shows the current strike ban threshold.
#[command(slash_command, required_permissions = "MANAGE_GUILD", ephemeral)]
async fn get(ctx: poise_error::Context<'_>) -> anyhow::Result<()> {
    let ban_threshold = BanThreshold::get_data_from(ctx.guild_id().expect(INVOCABLE_IN_GUILD))?;
    let description = match *ban_threshold {
        Some(ban_threshold) => &format!(
            "Currently, members are automatically banned if they accumulate **{ban_threshold} strikes**.",
        ),
        None => "The strike ban threshold is not set so members are not automatically banned.",
    };
    let strike_ban_threshold_set_command_mention = if let Some(command) = ctx
        .http()
        .get_global_commands()
        .await?
        .into_iter()
        .find(|command| command.name == "strike")
    {
        &format!("</strike ban_threshold set:{}>", command.id)
    } else {
        "/strike ban_threshold set"
    };

    ctx.send(
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(&[CreateComponent::Container(CreateContainer::new(&[
                CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "### Strike Ban Threshold\n{description}"
                ))),
                CreateComponent::Separator(CreateSeparator::new(true)),
                CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "-# You can change this with {strike_ban_threshold_set_command_mention}."
                ))),
            ]))]),
    )
    .await?;

    Ok(())
}

/// Sets the stike ban threshold.
#[command(slash_command, required_permissions = "MANAGE_GUILD", ephemeral)]
async fn set(
    ctx: poise_error::Context<'_>,
    #[description = "The number of strikes to set the threshold to. If omitted, members will never be auto-banned."]
    strikes: Option<usize>,
) -> anyhow::Result<()> {
    let guild_id = ctx.guild_id().expect(INVOCABLE_IN_GUILD);
    let current_ban_threshold = BanThreshold::get_data_from(guild_id)?;
    let new_ban_threshold = BanThreshold(strikes);

    if *new_ban_threshold == *current_ban_threshold {
        bail!(
            UserError::from_str("The current strike ban threshold is already set to this.")
                .unwrap()
        );
    }

    new_ban_threshold.set_data_for(guild_id)?;

    let description = match *new_ban_threshold {
        Some(ban_threshold) => &format!(
            "Now, members will be automatically banned if they accumulate **{ban_threshold} strikes**.",
        ),
        None => {
            "The strike ban threshold is no longer set so members will not be automatically banned."
        }
    };
    let main_text = CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
        "### Strike Ban Threshold\n{description}"
    )));

    ctx.send(
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(vec![CreateComponent::Container(CreateContainer::new(
                vec![main_text.clone()],
            ))]),
    )
    .await?;
    send_safety_alert(
        ctx,
        CreateMessage::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(&[CreateComponent::Container(CreateContainer::new(&[
                main_text,
                CreateComponent::Separator(CreateSeparator::new(true)),
                CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "-# Set by {} on <t:{}>.",
                    ctx.author().mention(),
                    UNIX_EPOCH
                        .elapsed()
                        .expect(UNIX_EPOCH_ELAPSED_ERR)
                        .as_secs(),
                ))),
            ]))])
            .allowed_mentions(CreateAllowedMentions::new()),
    )
    .await?;

    Ok(())
}

async fn ensure_author_has_perms(
    ctx: poise_error::Context<'_>,
    member: &Member,
) -> anyhow::Result<()> {
    let perms_for_seeing_others = serenity::Permissions::VIEW_AUDIT_LOG;

    if &member.user != ctx.author()
        && !ctx
            .author_member()
            .await
            .expect(INVOCABLE_IN_GUILD)
            .permissions
            .expect("command should only be invocable as a slash command")
            .contains(perms_for_seeing_others)
    {
        bail!(UserError::from(format!(
            "You must have permission to {perms_for_seeing_others} for info on other people's strikes.",
        )));
    }

    Ok(())
}

fn component_for_strike<'a>(
    member: &'a Member,
    strike_n: usize,
    strike: &'a Strike,
) -> anyhow::Result<CreateComponent<'a>> {
    let strikethrough = if strike.repeal.is_some() { "~~" } else { "" };
    let repeal_info = if let Some(repeal) = strike.repeal {
        &format!(
            "\n-# Repealed by {} on <t:{}>",
            repeal.user.mention(),
            repeal.timestamp,
        )
    } else {
        ""
    };

    Ok(CreateComponent::Container(CreateContainer::new(vec![
        CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
            "### {strikethrough}{} Strike {}{strikethrough}\n**Rule:** {}\n**Notes:** {}",
            member.mention(),
            strike_n,
            if let Some(rule_i) = strike.rule_i {
                let rules = Rules::get_data_from(member.guild_id)?;
                let rule_n = rule_i + 1;
                let rule = rules
                    .get(rule_i)
                    .ok_or(UserError::from(format!("There is no rule {rule_n}.")))?;

                format!("{rule_n}\\. {rule}")
            } else {
                "*None*".to_string()
            },
            strike.notes.as_deref().unwrap_or("*None*"),
        ))),
        CreateComponent::Separator(CreateSeparator::new(true)),
        CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
            "-# Issued by {} on <t:{}>{repeal_info}",
            strike.attribution.user.mention(),
            strike.attribution.timestamp,
        ))),
    ])))
}
