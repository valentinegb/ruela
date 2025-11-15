use std::ops::{Deref, DerefMut};

use crate::{
    data::{Attribution, Data, INVOCABLE_IN_GUILD},
    rule::{Rules, rule_autocomplete},
    util::ConfirmationPrompt,
};
use poise::{
    command,
    serenity_prelude::{
        CreateAllowedMentions, CreateComponent, CreateContainer, CreateMessage, CreateSeparator,
        CreateTextDisplay, Member, Mentionable, MessageFlags,
    },
};
use poise_error::{UserError, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Default)]
struct Convictions(Vec<Conviction>);

impl Data<Member> for Convictions {
    const PATH: &str = "convictions.cbor";
    const DESCRIPTOR: &str = "member convictions";
}

impl Deref for Convictions {
    type Target = Vec<Conviction>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Convictions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Deserialize, Serialize)]
struct Conviction {
    rule_i: Option<usize>,
    notes: Option<String>,
    attribution: Attribution,
    repeal: Option<Attribution>,
}

/// Declare someone to be guilty of an offense.
#[command(
    slash_command,
    interaction_context = "Guild",
    required_permissions = "MODERATE_MEMBERS",
    ephemeral
)]
pub async fn convict(
    ctx: poise_error::Context<'_>,
    #[description = "The person to convict."] member: Member,
    #[description = "The rule to convict the person of violating."]
    #[autocomplete = "rule_autocomplete"]
    #[rename = "rule"]
    rule_n: Option<u64>,
    #[description = "Any extra information that may be useful to include for the record."]
    notes: Option<String>,
) -> anyhow::Result<()> {
    let conviction = Conviction {
        // `rule_i` starts from 0, `rule_n` starts from 1.
        rule_i: rule_n.map(|rule_n| (rule_n - 1) as usize),
        notes,
        attribution: ctx.author().id.into(),
        repeal: None,
    };
    let mut convictions = Convictions::get_data_from(&member)?;
    let components = vec![CreateComponent::Container(CreateContainer::new(vec![
        CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
            "### {} Conviction {}\n**Rule:** {}\n**Notes:** {}",
            member.mention(),
            convictions.len() + 1,
            if let Some(rule_i) = conviction.rule_i {
                let rules = Rules::get_data_from(ctx.guild_id().expect(INVOCABLE_IN_GUILD))?;
                let rule_n = rule_i + 1;
                let rule = rules
                    .get(rule_i)
                    .ok_or(UserError::from(format!("There is no rule {rule_n}.")))?;

                format!("{rule_n}\\. {rule}")
            } else {
                "*None*".to_string()
            },
            conviction.notes.as_deref().unwrap_or("*None*"),
        ))),
        CreateComponent::Separator(CreateSeparator::new(true)),
        CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
            "-# Issued by {} on <t:{}>",
            conviction.attribution.user.mention(),
            conviction.attribution.timestamp,
        ))),
    ]))];
    let allowed_mentions = CreateAllowedMentions::new();
    let prompt = ConfirmationPrompt {
        components: components.clone(),
        prompt: Some("Are you sure you want issue this conviction?"),
        elaboration: Some("The convicted member will be notified and you  will be attributed."),
        confirm_text: Some("Issue it"),
        confirmed_text: Some("Conviction issued"),
        allowed_mentions: Some(allowed_mentions.clone()),
        ..Default::default()
    };
    let confirmed = prompt.prompt(ctx).await?;

    if confirmed {
        convictions.push(conviction);
        convictions.set_data_for(&member)?;

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

        let safety_alerts_channel_id = ctx
            .guild()
            .expect(INVOCABLE_IN_GUILD)
            .safety_alerts_channel_id;

        if let Some(safety_alerts_channel_id) = safety_alerts_channel_id {
            safety_alerts_channel_id
                .widen()
                .send_message(ctx.http(), message)
                .await?;
        }
    }

    Ok(())
}
