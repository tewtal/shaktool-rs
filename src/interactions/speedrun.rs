use poise::serenity_prelude::{
    ActionRowComponent, ComponentInteraction, Context, CreateActionRow, CreateInputText,
    CreateInteractionResponse, CreateInteractionResponseFollowup, CreateInteractionResponseMessage,
    CreateModal, EditMessage, InputTextStyle, Interaction, Member, ModalInteraction, RoleId,
};
use tracing::warn;

use crate::api::speedrun::{self, RunStatusChange};
use crate::tasks::speedrun::{
    dismiss_review, enter_review, outcome_verdict, resolve_demo, resolve_pending, verdict_embed,
    ReviewResult, RunOutcome,
};
use crate::{Data, Error};

const SCOPE: &str = "speedrun";

/// Handles the Approve/Reject review buttons on speedrun mod log messages.
pub async fn interaction_create_speedrun(
    ctx: &Context,
    interaction: &Interaction,
    data: &Data,
) -> Result<(), Error> {
    match interaction {
        Interaction::Component(component) => {
            if let Some(run_id) = component.data.custom_id.strip_prefix("speedrun_verify:") {
                handle_verify(ctx, component, data, run_id).await?;
            } else if let Some(run_id) = component.data.custom_id.strip_prefix("speedrun_reject:") {
                handle_reject_button(ctx, component, data, run_id, false).await?;
            } else if let Some(run_id) = component.data.custom_id.strip_prefix("speedrun_review:") {
                handle_review(ctx, component, data, run_id, false).await?;
            } else if let Some(run_id) = component.data.custom_id.strip_prefix("speedrun_dismiss:") {
                handle_dismiss(ctx, component, data, run_id, false).await?;
            } else if let Some(run_id) = component.data.custom_id.strip_prefix("speedrun_demo_verify:") {
                handle_demo_verify(ctx, component, data, run_id).await?;
            } else if let Some(run_id) = component.data.custom_id.strip_prefix("speedrun_demo_reject:") {
                handle_reject_button(ctx, component, data, run_id, true).await?;
            } else if let Some(run_id) = component.data.custom_id.strip_prefix("speedrun_demo_review:") {
                handle_review(ctx, component, data, run_id, true).await?;
            } else if let Some(run_id) = component.data.custom_id.strip_prefix("speedrun_demo_dismiss:") {
                handle_dismiss(ctx, component, data, run_id, true).await?;
            }
        }
        Interaction::Modal(modal) => {
            if let Some(run_id) = modal.data.custom_id.strip_prefix("speedrun_reject_modal:") {
                handle_reject_submit(ctx, modal, data, run_id, false).await?;
            } else if let Some(run_id) = modal.data.custom_id.strip_prefix("speedrun_demo_reject_modal:") {
                handle_reject_submit(ctx, modal, data, run_id, true).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn handle_verify(
    ctx: &Context,
    component: &ComponentInteraction,
    data: &Data,
    run_id: &str,
) -> Result<(), Error> {
    if !is_reviewer(data, component.member.as_ref()).await? {
        return deny(ctx, component).await;
    }
    let Some(api_key) = api_key() else {
        return fail(ctx, component, "SPEEDRUN_API_KEY is not configured.").await;
    };

    // Acknowledge before the API round-trips so Discord doesn't time out the
    // interaction; the mod log messages are edited by the resolution below.
    component.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await?;

    // The run may have been decided on the website (or by another moderator)
    // since this message was posted — sync instead of overriding blindly.
    if let Some(note) = preflight_decided(ctx, data, run_id).await? {
        return followup(ctx, component, &note).await;
    }

    if let Err(e) = speedrun::set_run_status(&api_key, run_id, &RunStatusChange::Verified).await {
        return followup(ctx, component, &format!("speedrun.com API call failed: {}", e)).await;
    }

    let outcome = RunOutcome::Approved { by: Some(component.user.name.clone()) };
    apply_outcome(ctx, data, component, run_id, &outcome).await
}

/// Checks whether the run is still pending on speedrun.com. If it was
/// already decided (or deleted), the mod log is synced to that outcome and
/// a note for the clicking moderator is returned. A failed lookup returns
/// `None` so the action proceeds as before.
async fn preflight_decided(ctx: &Context, data: &Data, run_id: &str) -> Result<Option<String>, Error> {
    let decided = match speedrun::get_run(run_id).await {
        Ok(Some(run)) => match run.status.status.as_str() {
            "verified" => Some((
                RunOutcome::Approved { by: None },
                "This run was already approved on speedrun.com.".to_string(),
            )),
            "rejected" => Some((
                RunOutcome::Rejected { by: None, reason: run.status.reason.clone() },
                "This run was already rejected on speedrun.com.".to_string(),
            )),
            _ => None,
        },
        Ok(None) => Some((RunOutcome::Removed, "This run no longer exists on speedrun.com.".to_string())),
        Err(e) => {
            warn!("Speedrun review: pre-flight check for run {} failed: {:?}", run_id, e);
            None
        }
    };
    let Some((outcome, note)) = decided else {
        return Ok(None);
    };
    resolve_pending(ctx, &data.db, run_id, &outcome).await?;
    Ok(Some(note))
}

/// Demo approval: updates messages and announces, never touches speedrun.com.
async fn handle_demo_verify(
    ctx: &Context,
    component: &ComponentInteraction,
    data: &Data,
    run_id: &str,
) -> Result<(), Error> {
    if !is_reviewer(data, component.member.as_ref()).await? {
        return deny(ctx, component).await;
    }
    component.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await?;

    let outcome = RunOutcome::Approved { by: Some(component.user.name.clone()) };
    if !resolve_demo(ctx, &data.db, run_id, &outcome).await? {
        let message = &component.message;
        if !edit_untracked(ctx, &message.channel_id, message.id, &outcome, true).await {
            followup(ctx, component, "This run was already handled by another moderator.").await?;
        }
    }
    Ok(())
}

/// Opens a review thread on a run. Shared by the real and demo buttons; the
/// demo flag selects which tracking entry and button namespace to use.
async fn handle_review(
    ctx: &Context,
    component: &ComponentInteraction,
    data: &Data,
    run_id: &str,
    demo: bool,
) -> Result<(), Error> {
    if !is_reviewer(data, component.member.as_ref()).await? {
        return deny(ctx, component).await;
    }
    component.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await?;

    let result = enter_review(ctx, &data.db, run_id, &component.user.name, demo).await?;
    let note = match result {
        ReviewResult::Opened(thread_id) => format!("🔍 Review opened: <#{}>", thread_id),
        ReviewResult::AlreadyOpen(thread_id) => {
            format!("This run is already under review: <#{}>", thread_id)
        }
        ReviewResult::NotTracked => {
            "This run is no longer tracked, so a review can't be opened.".to_string()
        }
        ReviewResult::Failed => {
            "Couldn't open a review thread — the mod log channel may not support threads."
                .to_string()
        }
    };
    followup(ctx, component, &note).await
}

/// Dismisses an open review, returning the run to the queue. Posted on the
/// in-thread button.
async fn handle_dismiss(
    ctx: &Context,
    component: &ComponentInteraction,
    data: &Data,
    run_id: &str,
    demo: bool,
) -> Result<(), Error> {
    if !is_reviewer(data, component.member.as_ref()).await? {
        return deny(ctx, component).await;
    }
    component.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await?;

    if !dismiss_review(ctx, &data.db, run_id, &component.user.name, demo).await? {
        followup(ctx, component, "This run isn't under review (it may already be resolved).").await?;
    }
    Ok(())
}

async fn handle_reject_button(
    ctx: &Context,
    component: &ComponentInteraction,
    data: &Data,
    run_id: &str,
    demo: bool,
) -> Result<(), Error> {
    if !is_reviewer(data, component.member.as_ref()).await? {
        return deny(ctx, component).await;
    }

    let modal_id = if demo { "speedrun_demo_reject_modal" } else { "speedrun_reject_modal" };
    let modal = CreateModal::new(format!("{}:{}", modal_id, run_id), "Reject run")
        .components(vec![CreateActionRow::InputText(
            CreateInputText::new(InputTextStyle::Paragraph, "Reason", "reason")
                .placeholder("Shown to the runner on speedrun.com")
                .required(true),
        )]);
    component.create_response(&ctx.http, CreateInteractionResponse::Modal(modal)).await?;
    Ok(())
}

async fn handle_reject_submit(
    ctx: &Context,
    modal: &ModalInteraction,
    data: &Data,
    run_id: &str,
    demo: bool,
) -> Result<(), Error> {
    if !is_reviewer(data, modal.member.as_ref()).await? {
        return Ok(());
    }
    let api_key = api_key();
    if !demo && api_key.is_none() {
        return modal_fail(ctx, modal, "SPEEDRUN_API_KEY is not configured.").await;
    }
    let reason = modal
        .data
        .components
        .iter()
        .flat_map(|row| row.components.iter())
        .find_map(|component| match component {
            ActionRowComponent::InputText(input) => input.value.clone(),
            _ => None,
        })
        .unwrap_or_else(|| "Rejected by a moderator.".to_string());

    modal.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await?;

    if !demo {
        if let Some(note) = preflight_decided(ctx, data, run_id).await? {
            return modal_followup(ctx, modal, &note).await;
        }
        let change = RunStatusChange::Rejected { reason: reason.clone() };
        let api_key = api_key
            .as_deref()
            .expect("non-demo rejection checked for SPEEDRUN_API_KEY");
        if let Err(e) = speedrun::set_run_status(api_key, run_id, &change).await {
            return modal_followup(ctx, modal, &format!("speedrun.com API call failed: {}", e)).await;
        }
    }

    let outcome = RunOutcome::Rejected { by: Some(modal.user.name.clone()), reason: Some(reason) };
    let resolved = if demo {
        resolve_demo(ctx, &data.db, run_id, &outcome).await?
    } else {
        resolve_pending(ctx, &data.db, run_id, &outcome).await?
    };
    if !resolved {
        if let Some(message) = &modal.message {
            if !edit_untracked(ctx, &message.channel_id, message.id, &outcome, demo).await {
                modal_followup(ctx, modal, "This run was already handled by another moderator.").await?;
            }
        }
    }
    Ok(())
}

async fn apply_outcome(
    ctx: &Context,
    data: &Data,
    component: &ComponentInteraction,
    run_id: &str,
    outcome: &RunOutcome,
) -> Result<(), Error> {
    if !resolve_pending(ctx, &data.db, run_id, outcome).await? {
        let message = &component.message;
        if !edit_untracked(ctx, &message.channel_id, message.id, outcome, false).await {
            followup(ctx, component, "This run was already handled by another moderator.").await?;
        }
    }
    Ok(())
}

/// Fallback when no tracked entry was claimed: the message is either a
/// legacy untracked one (still has its buttons — edit it in place) or was
/// just resolved by a concurrent reviewer (buttons already stripped — leave
/// it alone). Returns whether an edit was made.
async fn edit_untracked(
    ctx: &Context,
    channel_id: &poise::serenity_prelude::ChannelId,
    message_id: poise::serenity_prelude::MessageId,
    outcome: &RunOutcome,
    demo: bool,
) -> bool {
    let fresh = match channel_id.message(&ctx.http, message_id).await {
        Ok(message) => message,
        Err(e) => {
            warn!("Speedrun review: fetching message {} failed: {:?}", message_id, e);
            return true;
        }
    };
    if fresh.components.is_empty() {
        return false;
    }

    let (colour, mut verdict) = outcome_verdict(outcome);
    if demo {
        verdict.push_str(" — 🧪 demo, no speedrun.com action");
    }
    let embed = verdict_embed(fresh.embeds.first(), colour, &verdict);
    let edit = EditMessage::new().embed(embed).components(vec![]);
    if let Err(e) = channel_id.edit_message(&ctx.http, message_id, edit).await {
        warn!("Speedrun review: editing message {} failed: {:?}", message_id, e);
    }
    true
}

/// Review buttons may be used by administrators, or by members holding any of
/// the roles configured in this server's `speedrun.mod_role` (a comma-separated
/// list of role ids).
async fn is_reviewer(data: &Data, member: Option<&Member>) -> Result<bool, Error> {
    let Some(member) = member else {
        return Ok(false);
    };
    if member.permissions.is_some_and(|p| p.administrator()) {
        return Ok(true);
    }
    if let Some(setting) = data.db.get_guild_setting(member.guild_id.get(), SCOPE, "mod_role").await? {
        let allowed = setting
            .split(',')
            .map(str::trim)
            .filter_map(|id| id.parse::<u64>().ok())
            .any(|id| member.roles.contains(&RoleId::new(id)));
        return Ok(allowed);
    }
    Ok(false)
}

fn api_key() -> Option<String> {
    std::env::var("SPEEDRUN_API_KEY")
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

async fn deny(ctx: &Context, component: &ComponentInteraction) -> Result<(), Error> {
    fail(ctx, component, "You don't have permission to review runs.").await
}

async fn fail(ctx: &Context, component: &ComponentInteraction, text: &str) -> Result<(), Error> {
    let response = CreateInteractionResponseMessage::new().content(text).ephemeral(true);
    component.create_response(&ctx.http, CreateInteractionResponse::Message(response)).await?;
    Ok(())
}

async fn followup(ctx: &Context, component: &ComponentInteraction, text: &str) -> Result<(), Error> {
    let message = CreateInteractionResponseFollowup::new().content(text).ephemeral(true);
    component.create_followup(&ctx.http, message).await?;
    Ok(())
}

async fn modal_followup(ctx: &Context, modal: &ModalInteraction, text: &str) -> Result<(), Error> {
    let message = CreateInteractionResponseFollowup::new().content(text).ephemeral(true);
    modal.create_followup(&ctx.http, message).await?;
    Ok(())
}

async fn modal_fail(ctx: &Context, modal: &ModalInteraction, text: &str) -> Result<(), Error> {
    let response = CreateInteractionResponseMessage::new().content(text).ephemeral(true);
    modal.create_response(&ctx.http, CreateInteractionResponse::Message(response)).await?;
    Ok(())
}
