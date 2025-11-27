use std::time::Duration;

use poise::{
    CreateReply,
    serenity_prelude::{
        ButtonStyle, CollectComponentInteractions, CreateActionRow, CreateAllowedMentions,
        CreateButton, CreateComponent, CreateContainer, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateMessage, CreateSeparator, CreateTextDisplay,
        MessageFlags, Spacing,
        colours::css::{DANGER, POSITIVE, WARNING},
        small_fixed_array::FixedString,
    },
};
use poise_error::anyhow::{self, Context};

use crate::data::INVOCABLE_IN_GUILD;

/// Should be used before actions which are irreversible. Reversible actions do
/// not need confirmation.
#[derive(Default)]
pub struct ConfirmationPrompt<'a> {
    pub components: Vec<CreateComponent<'a>>,
    pub prompt: Option<&'a str>,
    pub elaboration: Option<&'a str>,
    pub cancel_text: Option<&'a str>,
    pub confirm_text: Option<&'a str>,
    pub canceled_text: Option<&'a str>,
    pub confirmed_text: Option<&'a str>,
    pub allowed_mentions: Option<CreateAllowedMentions<'a>>,
}

impl<'a> ConfirmationPrompt<'a> {
    fn prompt_component(&self, timed_out: bool) -> CreateComponent<'_> {
        let mut container_components = vec![
            CreateComponent::TextDisplay(CreateTextDisplay::new(format!(
                "**{}**\n{}This action is irreversible.",
                self.prompt.unwrap_or("Are you sure?"),
                self.elaboration
                    .map(|elaboration| format!("{elaboration} "))
                    .unwrap_or_default(),
            ))),
            CreateComponent::ActionRow(CreateActionRow::buttons(vec![
                CreateButton::new("cancel")
                    .label(self.cancel_text.unwrap_or("Cancel"))
                    .style(ButtonStyle::Secondary)
                    .disabled(timed_out),
                CreateButton::new("confirm")
                    .label(self.confirm_text.unwrap_or("Confirm"))
                    .style(ButtonStyle::Danger)
                    .disabled(timed_out),
            ])),
        ];

        if timed_out {
            container_components.push(CreateComponent::TextDisplay(CreateTextDisplay::new(
                "You've run out of time to choose.",
            )));
        }

        CreateComponent::Container(CreateContainer::new(container_components).accent_color(WARNING))
    }

    fn canceled_component(&self) -> CreateComponent<'_> {
        CreateComponent::Container(
            CreateContainer::new(vec![CreateComponent::TextDisplay(CreateTextDisplay::new(
                format!("**{}**", self.canceled_text.unwrap_or("Canceled"),),
            ))])
            .accent_color(DANGER),
        )
    }

    fn confirmed_component(&self) -> CreateComponent<'_> {
        CreateComponent::Container(
            CreateContainer::new(vec![CreateComponent::TextDisplay(CreateTextDisplay::new(
                format!("**{}**", self.confirmed_text.unwrap_or("Confirmed"),),
            ))])
            .accent_color(POSITIVE),
        )
    }

    fn create_reply(&'a self, component: CreateComponent<'a>) -> CreateReply<'a> {
        let mut components = vec![CreateComponent::TextDisplay(CreateTextDisplay::new(
            "-# *Preview of Changes*",
        ))];

        components.extend(self.components.clone());
        components.push(CreateComponent::Separator(
            CreateSeparator::new(true).spacing(Spacing::Large),
        ));
        components.push(component);

        let mut reply = CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components);

        if let Some(allowed_mentions) = self.allowed_mentions.clone() {
            reply = reply.allowed_mentions(allowed_mentions);
        }

        reply
    }

    pub async fn prompt(&'a self, ctx: poise_error::Context<'a>) -> anyhow::Result<bool> {
        let reply_handle = ctx
            .send(self.create_reply(self.prompt_component(false)))
            .await?;

        match reply_handle
            .message()
            .await?
            .id
            .collect_component_interactions(ctx.serenity_context())
            .author_id(ctx.author().id)
            .custom_ids(
                [
                    FixedString::from_static_trunc("cancel"),
                    FixedString::from_static_trunc("confirm"),
                ]
                .into(),
            )
            // TODO: Uncomment when at least 1.91.0 makes it to Nix 25.05
            // .timeout(Duration::from_mins(1))
            .timeout(Duration::from_secs(60))
            .await
        {
            Some(interaction) => match interaction.data.custom_id.as_str() {
                "cancel" => {
                    interaction
                        .create_response(
                            ctx.http(),
                            CreateInteractionResponse::UpdateMessage(
                                self.create_reply(self.canceled_component())
                                    .to_slash_initial_response(
                                        CreateInteractionResponseMessage::new(),
                                    ),
                            ),
                        )
                        .await?;
                }
                "confirm" => {
                    interaction
                        .create_response(
                            ctx.http(),
                            CreateInteractionResponse::UpdateMessage(
                                self.create_reply(self.confirmed_component())
                                    .to_slash_initial_response(
                                        CreateInteractionResponseMessage::new(),
                                    ),
                            ),
                        )
                        .await?;

                    return Ok(true);
                }
                _ => unreachable!(),
            },
            None => {
                reply_handle
                    .edit(ctx, self.create_reply(self.prompt_component(true)))
                    .await?;
            }
        }

        Ok(false)
    }
}

pub async fn send_safety_alert(
    ctx: poise_error::Context<'_>,
    message: CreateMessage<'_>,
) -> anyhow::Result<()> {
    let safety_alerts_channel_id = ctx
        .guild()
        .expect(INVOCABLE_IN_GUILD)
        .safety_alerts_channel_id;

    if let Some(safety_alerts_channel_id) = safety_alerts_channel_id {
        safety_alerts_channel_id
            .widen()
            .send_message(ctx.http(), message)
            .await
            .context("could not send safety alert")?;
    }

    Ok(())
}
