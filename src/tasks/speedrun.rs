use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use async_trait::async_trait;
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::{
    AutoArchiveDuration, ButtonStyle, ChannelId, CreateActionRow, CreateButton, CreateEmbed,
    CreateEmbedFooter, CreateMessage, CreateThread, EditMessage, EditThread, Embed, MessageId,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::{Task, TaskContext};
use crate::api::speedrun::{self, Run, RunStatusChange};
use crate::db::Db;
use crate::Error;

pub mod judge;
mod demo;

use judge::{Evidence, Judge, Judgement, RuleJudge};

const TASK_NAME: &str = "speedrun_monitor";
const SCOPE: &str = "speedrun";

/// Suspicion score (0-100) at or above which a run is treated as suspicious.
/// Overridable with the global `speedrun.threshold` setting and per
/// game/category with `speedrun.thresholds`.
const DEFAULT_THRESHOLD: u32 = 50;

/// New queue runs processed per game per tick; evidence gathering costs a few
/// API calls per run, so a large backlog is drained over several ticks.
const MAX_NEW_RUNS_PER_TICK: usize = 10;

const COLOUR_PENDING: u32 = 0x3498DB;
const COLOUR_FLAGGED: u32 = 0xE67E22;
const COLOUR_APPROVED: u32 = 0x2ECC71;
const COLOUR_REJECTED: u32 = 0x992D22;
const COLOUR_REMOVED: u32 = 0x95A5A6;
const COLOUR_REVIEW: u32 = 0x9B59B6;

/// Embed colour for a run still in the mod log: only an `auto`-mode run that
/// scored suspicious is highlighted; everything else stays neutral, since in
/// manual mode the assessment is informational rather than an accusation.
fn pending_colour(mode: Mode, suspicious: bool) -> u32 {
    match (mode, suspicious) {
        (Mode::Auto, true) => COLOUR_FLAGGED,
        _ => COLOUR_PENDING,
    }
}

/// How submissions to a game (or one of its categories) are moderated.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// Every queue run is posted to the mod log for human review; the bot
    /// takes no action on its own.
    Manual,
    /// Clean runs are auto-approved; suspicious runs are left in the queue
    /// for human review.
    Auto,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "manual" => Some(Mode::Manual),
            "auto" => Some(Mode::Auto),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Manual => "manual",
            Mode::Auto => "auto",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum PlannedAction {
    /// Auto mode, clean run: approve it.
    Verify,
    /// Post to the mod log and wait for a human (or the website) to decide.
    AwaitReview,
}

/// How a pending run left the queue.
pub enum RunOutcome {
    /// `by` is the Discord reviewer; `None` means it happened on the website.
    Approved { by: Option<String> },
    Rejected { by: Option<String>, reason: Option<String> },
    Removed,
}

/// A queue run the bot has posted to mod logs and is watching for outcome.
#[derive(Serialize, Deserialize)]
struct PendingRun {
    game: String,
    /// `(channel_id, message_id)` of every mod log message for this run.
    messages: Vec<(u64, u64)>,
    /// Demo entries pin their announce channels here; real entries leave this
    /// empty and the channels are looked up from settings at resolution time.
    #[serde(default)]
    announce_channels: Vec<u64>,
    /// Showcase runs don't exist on speedrun.com, so their announcement
    /// content is stored up front instead of fetched at approval time.
    #[serde(default)]
    demo_announcement: Option<DemoAnnouncement>,
    /// `Some` while the run is under discussion in a review thread. A
    /// Discord-only state: the run stays in the speedrun.com queue.
    #[serde(default)]
    review: Option<ReviewState>,
}

/// An open review thread on a pending run. Created by a moderator who wants
/// discussion before deciding; resolving the run (or dismissing the review)
/// archives the thread.
#[derive(Serialize, Deserialize)]
struct ReviewState {
    /// The discussion thread spawned off the first mod log message.
    thread_id: u64,
    /// `(channel_id, message_id)` of the thread's button message, so its
    /// components can be stripped when the review ends.
    thread_message: Option<(u64, u64)>,
    /// Name of the moderator who opened the review.
    opened_by: String,
    /// Status note shown on the mod log embeds before review opened, restored
    /// verbatim if the review is dismissed.
    prior_status: String,
    /// Embed colour before review opened, restored on dismiss.
    prior_colour: u32,
}

/// Pre-baked announcement for a fake showcase run.
#[derive(Serialize, Deserialize)]
struct DemoAnnouncement {
    game_name: String,
    category: String,
    time: String,
    players: String,
    weblink: String,
    comment: Option<String>,
    videos: Vec<String>,
    blurbs: Vec<String>,
}

/// Outcome of posting a demo batch.
pub enum DemoResult {
    Posted(usize),
    NoModChannel,
    UnknownGame,
}

pub struct SpeedrunDebugReport {
    pub abbreviation: String,
    pub game_name: String,
    pub default_mode: Mode,
    pub default_threshold: u32,
    pub requested_count: usize,
    pub results: Vec<SpeedrunDebugResult>,
}

pub struct SpeedrunDebugResult {
    pub run_id: String,
    pub weblink: String,
    pub status: String,
    pub submitted: String,
    pub category: String,
    pub time: String,
    pub players: String,
    pub mode: Option<&'static str>,
    pub threshold: Option<u32>,
    pub score: Option<u32>,
    pub suspicious: Option<bool>,
    pub action: String,
    pub reasons: Vec<String>,
    pub skipped: Option<String>,
}

/// Global moderation policy, with per-game and per-category overrides. Keys
/// are `game` or `game/category`, lowercased.
pub struct Policy {
    modes: HashMap<String, Mode>,
    thresholds: HashMap<String, u32>,
    default_threshold: u32,
    pub dry_run: bool,
}

impl Policy {
    pub fn mode_for(&self, game: &str, category: &str) -> Mode {
        lookup_override(&self.modes, game, category).unwrap_or(Mode::Manual)
    }

    pub fn threshold_for(&self, game: &str, category: &str) -> u32 {
        lookup_override(&self.thresholds, game, category).unwrap_or(self.default_threshold)
    }
}

/// Most specific match wins: `game/category`, then `game`.
fn lookup_override<T: Copy>(map: &HashMap<String, T>, game: &str, category: &str) -> Option<T> {
    let game = game.to_lowercase();
    map.get(&format!("{}/{}", game, category.to_lowercase()))
        .or_else(|| map.get(&game))
        .copied()
}

/// Everything needed to process one configured game's submissions.
struct GameContext<'a> {
    abbreviation: &'a str,
    game_name: String,
    game_id: String,
    /// Moderation log channels (queue runs with review buttons), per server.
    mod_channels: Vec<ChannelId>,
    /// Public channels where approved runs are announced, per server.
    announce_channels: Vec<ChannelId>,
    api_key: Option<&'a str>,
    policy: &'a Policy,
}

struct RunPipelineResult {
    evidence: Evidence,
    judgement: Judgement,
    mode: Mode,
    threshold: u32,
    suspicious: bool,
    action: PlannedAction,
}

/// Watches the speedrun.com verification queue for configured games:
///
/// - every queue run is judged (player history, video checks, leaderboard
///   context) and posted to each server's moderation log channel with
///   Approve/Reject buttons
/// - the queue is tracked: runs approved/rejected/removed on the website have
///   their mod log messages updated, and approvals are announced
/// - approved runs (via button, website, or auto mode) are announced in each
///   server's public announce channel; approved runs are not tracked further
/// - in `auto` mode clean runs are auto-approved; suspicious ones stay queued
///
/// Per-server configuration (via the `config` command):
/// - `config set speedrun mod_channel <channel id>` — moderation log
/// - `config set speedrun announce_channel <channel id>` — public announcements
/// - `config set speedrun games <abbreviation>,...` — games this server watches
/// - `config set speedrun mod_role <role id>` (optional; role allowed to review)
///
/// Global configuration (moderation policy, shared by all servers):
/// - `config set speedrun modes <game[/category]>:<manual|auto>,...` (default manual)
/// - `config set speedrun threshold <0-100>` (default 50)
/// - `config set speedrun thresholds <game[/category]>:<0-100>,...` (overrides)
/// - `config set speedrun dry_run true` (report what would be done without doing it)
///
/// Review buttons and `auto` mode require the `SPEEDRUN_API_KEY` environment
/// variable, holding the API key of a game moderator account.
pub struct SpeedrunMonitor {
    judge: Box<dyn Judge>,
}

impl SpeedrunMonitor {
    pub fn new() -> Self {
        // Swap or chain judges here, e.g. an LLM-backed `Judge` consuming the
        // serialized `Evidence`.
        SpeedrunMonitor { judge: Box::new(RuleJudge) }
    }
}

#[async_trait]
impl Task for SpeedrunMonitor {
    fn name(&self) -> &'static str {
        TASK_NAME
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(120)
    }

    async fn run(&self, task_ctx: &TaskContext) -> Result<(), Error> {
        let db = &task_ctx.db;
        let policy = load_policy(db).await?;
        let api_key = std::env::var("SPEEDRUN_API_KEY").ok();

        // Per-server subscriptions: game -> mod log / announce channels.
        let mut mod_channels: HashMap<String, Vec<ChannelId>> = HashMap::new();
        let mut announce_channels: HashMap<String, Vec<ChannelId>> = HashMap::new();
        for (guild_id, games) in db.guild_setting_values(SCOPE, "games").await? {
            let mod_channel = parse_channel(db.get_guild_setting(guild_id, SCOPE, "mod_channel").await?);
            let announce_channel =
                parse_channel(db.get_guild_setting(guild_id, SCOPE, "announce_channel").await?);
            for abbreviation in games.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let abbreviation = abbreviation.to_lowercase();
                if let Some(channel) = mod_channel {
                    mod_channels.entry(abbreviation.clone()).or_default().push(channel);
                }
                if let Some(channel) = announce_channel {
                    announce_channels.entry(abbreviation).or_default().push(channel);
                }
            }
        }

        // Process every game any server watches, plus games with a mode set.
        let games: BTreeSet<String> = mod_channels
            .keys()
            .chain(announce_channels.keys())
            .cloned()
            .chain(policy.modes.keys().filter_map(|k| k.split('/').next().map(str::to_string)))
            .collect();

        for abbreviation in &games {
            let Some((game_id, game_name)) = self.resolve_game(db, abbreviation).await? else {
                continue;
            };
            let game = GameContext {
                abbreviation,
                game_name,
                game_id,
                mod_channels: mod_channels.get(abbreviation).cloned().unwrap_or_default(),
                announce_channels: announce_channels.get(abbreviation).cloned().unwrap_or_default(),
                api_key: api_key.as_deref(),
                policy: &policy,
            };

            if let Err(e) = self.check_game(task_ctx, &game).await {
                warn!("Speedrun monitor: checking '{}' failed: {:?}", abbreviation, e);
            }
        }

        Ok(())
    }
}

impl SpeedrunMonitor {
    pub async fn debug_recent_runs(
        &self,
        db: &Db,
        abbreviation: &str,
        count: usize,
        mode_override: Option<Mode>,
    ) -> Result<Option<SpeedrunDebugReport>, Error> {
        let count = count.clamp(1, 200);
        let policy = load_policy(db).await?;
        let Some((game_id, game_name)) = self.resolve_game(db, abbreviation).await? else {
            return Ok(None);
        };
        let game = GameContext {
            abbreviation,
            game_name: game_name.clone(),
            game_id,
            mod_channels: Vec::new(),
            announce_channels: Vec::new(),
            api_key: None,
            policy: &policy,
        };

        let runs = speedrun::get_runs_limited(&game.game_id, None, count).await?;
        let mut results = Vec::new();
        for run in &runs {
            results.push(self.debug_run(&game, run, mode_override).await?);
        }

        Ok(Some(SpeedrunDebugReport {
            abbreviation: abbreviation.to_string(),
            game_name,
            default_mode: policy.mode_for(abbreviation, ""),
            default_threshold: policy.threshold_for(abbreviation, ""),
            requested_count: count,
            results,
        }))
    }

    /// Posts entirely fabricated submissions to one server's mod log,
    /// showcasing the different flags and scores. See [`demo`].
    pub async fn demo_showcase(
        &self,
        ctx: &serenity::Context,
        db: &Db,
        guild_id: u64,
    ) -> Result<DemoResult, Error> {
        demo::demo_showcase(self, ctx, db, guild_id).await
    }

    /// Posts the latest real submissions to one server's channels as a demo of
    /// the moderation flow, without ever touching speedrun.com. See [`demo`].
    pub async fn demo_recent_runs(
        &self,
        ctx: &serenity::Context,
        db: &Db,
        guild_id: u64,
        abbreviation: &str,
        count: usize,
    ) -> Result<DemoResult, Error> {
        demo::demo_recent_runs(self, ctx, db, guild_id, abbreviation, count).await
    }

    /// Resolves a game abbreviation to its id and name, cached in task state.
    async fn resolve_game(
        &self,
        db: &Db,
        abbreviation: &str,
    ) -> Result<Option<(String, String)>, Error> {
        let id_key = format!("game_id:{}", abbreviation);
        let name_key = format!("game_name:{}", abbreviation);

        if let (Some(id), Some(name)) = (
            db.get_task_state(TASK_NAME, &id_key).await?,
            db.get_task_state(TASK_NAME, &name_key).await?,
        ) {
            return Ok(Some((id, name)));
        }

        match speedrun::get_game(abbreviation).await? {
            Some(game) => {
                db.set_task_state(TASK_NAME, &id_key, &game.id).await?;
                db.set_task_state(TASK_NAME, &name_key, &game.names.international).await?;
                Ok(Some((game.id, game.names.international)))
            }
            None => {
                warn!("Speedrun monitor: no game found for abbreviation '{}'", abbreviation);
                Ok(None)
            }
        }
    }

    async fn check_game(&self, task_ctx: &TaskContext, game: &GameContext<'_>) -> Result<(), Error> {
        let db = &task_ctx.db;
        let queue = speedrun::get_runs(&game.game_id, Some("new")).await?;
        let queue_ids: HashSet<&str> = queue.iter().map(|r| r.id.as_str()).collect();

        // Tracked runs that left the queue were decided on the website (or
        // deleted by the runner): update their mod log messages and announce
        // approvals.
        for (key, raw) in db.list_task_state(TASK_NAME, "pending:").await? {
            let Ok(pending) = serde_json::from_str::<PendingRun>(&raw) else {
                db.delete_task_state(TASK_NAME, &key).await?;
                continue;
            };
            if pending.game != game.abbreviation {
                continue;
            }
            let Some(run_id) = key.strip_prefix("pending:") else { continue };
            if queue_ids.contains(run_id) {
                continue;
            }
            let outcome = match speedrun::get_run(run_id).await? {
                None => RunOutcome::Removed,
                Some(run) => match run.status.status.as_str() {
                    "verified" => RunOutcome::Approved { by: None },
                    "rejected" => RunOutcome::Rejected { by: None, reason: run.status.reason.clone() },
                    // Still in the queue (fetch window missed it); keep waiting.
                    _ => continue,
                },
            };
            if let Err(e) = resolve_pending(&task_ctx.ctx, db, run_id, &outcome).await {
                warn!("Speedrun monitor: resolving run {} failed: {:?}", run_id, e);
            }
        }

        // New queue runs, oldest first.
        let mut processed = 0;
        for run in queue.iter().rev() {
            let seen_key = format!("seen:{}", run.id);
            if db.get_task_state(TASK_NAME, &seen_key).await?.is_some() {
                continue;
            }
            if processed >= MAX_NEW_RUNS_PER_TICK {
                break;
            }
            processed += 1;
            db.set_task_state(TASK_NAME, &seen_key, "1").await?;
            if let Err(e) = self.process_run(task_ctx, game, run).await {
                warn!("Speedrun monitor: processing run {} failed: {:?}", run.id, e);
            }
        }

        Ok(())
    }

    async fn process_run(
        &self,
        task_ctx: &TaskContext,
        game: &GameContext<'_>,
        run: &Run,
    ) -> Result<(), Error> {
        let db = &task_ctx.db;
        let result = self.evaluate_run(game, run, None).await?;
        let dry_run = game.policy.dry_run;
        let can_act = game.api_key.is_some() && !dry_run;

        // Auto mode approves clean runs immediately; everything else waits
        // for review.
        if result.action == PlannedAction::Verify && can_act {
            match speedrun::set_run_status(game.api_key.unwrap_or_default(), &run.id, &RunStatusChange::Verified).await {
                Ok(()) => {
                    let embed = build_mod_embed(game, run, &result, COLOUR_APPROVED, "✅ Auto-approved");
                    post_to_channels(&task_ctx.ctx, &game.mod_channels, &embed, None).await;
                    announce_approved(&task_ctx.ctx, &game.game_name, &game.game_id, run, &game.announce_channels, false).await;
                    return Ok(());
                }
                Err(e) => {
                    warn!("Speedrun monitor: auto-approve of run {} failed: {:?}", run.id, e);
                    // Fall through to manual handling below.
                }
            }
        }

        let status_note = match (result.action, result.mode, dry_run) {
            (PlannedAction::Verify, _, true) => {
                "🧪 Dry run — would auto-approve this run; buttons simulate the flow"
            }
            (PlannedAction::Verify, _, false) if game.api_key.is_none() => {
                "⏳ Would auto-approve, but SPEEDRUN_API_KEY is not set — needs manual review"
            }
            (PlannedAction::Verify, _, false) => "⏳ Auto-approve failed — needs manual review",
            (PlannedAction::AwaitReview, Mode::Auto, true) => {
                "🧪 Dry run — would be flagged for manual review; buttons simulate the flow"
            }
            (PlannedAction::AwaitReview, Mode::Auto, false) => "⚠️ Flagged — left in queue for manual review",
            (PlannedAction::AwaitReview, Mode::Manual, true) => {
                "🧪 Dry run — pending review; buttons simulate the flow"
            }
            (PlannedAction::AwaitReview, Mode::Manual, false) => "⏳ Pending review",
        };
        let colour = pending_colour(result.mode, result.suspicious);

        let embed = build_mod_embed(game, run, &result, colour, status_note);
        // Dry run posts simulated buttons: they update messages and announce,
        // but their handlers never call speedrun.com.
        let buttons = if dry_run {
            Some(review_buttons(&run.id, true))
        } else if can_act {
            Some(review_buttons(&run.id, false))
        } else {
            None
        };
        let messages = post_to_channels(&task_ctx.ctx, &game.mod_channels, &embed, buttons).await;

        // Track the run even with no mod log messages so website outcomes
        // still trigger announcements.
        let pending = PendingRun {
            game: game.abbreviation.to_string(),
            messages,
            announce_channels: Vec::new(),
            demo_announcement: None,
            review: None,
        };
        db.set_task_state(TASK_NAME, &format!("pending:{}", run.id), &serde_json::to_string(&pending)?)
            .await?;

        Ok(())
    }

    async fn debug_run(
        &self,
        game: &GameContext<'_>,
        run: &Run,
        mode_override: Option<Mode>,
    ) -> Result<SpeedrunDebugResult, Error> {
        let mut result = SpeedrunDebugResult {
            run_id: run.id.clone(),
            weblink: run.weblink.clone(),
            status: run.status.status.clone(),
            submitted: run.submitted.clone().unwrap_or_else(|| "unknown".to_string()),
            category: run.category.data.name.clone(),
            time: run.formatted_time(),
            players: run.player_names(),
            mode: None,
            threshold: None,
            score: None,
            suspicious: None,
            action: "Dry run - no action".to_string(),
            reasons: Vec::new(),
            skipped: None,
        };

        let pipeline = self.evaluate_run(game, run, mode_override).await?;
        result.mode = Some(pipeline.mode.label());
        result.threshold = Some(pipeline.threshold);
        result.score = Some(pipeline.judgement.score);
        result.suspicious = Some(pipeline.suspicious);
        result.reasons = pipeline.judgement.reasons;
        result.action = if run.status.status == "new" {
            match pipeline.action {
                PlannedAction::Verify => "Would auto-approve (auto mode, score below threshold)".to_string(),
                PlannedAction::AwaitReview if pipeline.mode == Mode::Auto => {
                    "Would leave in queue for manual review (flagged)".to_string()
                }
                PlannedAction::AwaitReview => "Would post to the mod log for manual review".to_string(),
            }
        } else {
            format!("n/a — run is '{}'; moderation only processes queue runs", run.status.status)
        };
        Ok(result)
    }

    async fn evaluate_run(
        &self,
        game: &GameContext<'_>,
        run: &Run,
        mode_override: Option<Mode>,
    ) -> Result<RunPipelineResult, Error> {
        let category = &run.category.data.name;
        let mode = mode_override.unwrap_or_else(|| game.policy.mode_for(game.abbreviation, category));
        let threshold = game.policy.threshold_for(game.abbreviation, category);
        let evidence = judge::gather(game.abbreviation, &game.game_name, &game.game_id, run).await;
        let judgement = self.judge.judge(&evidence).await?;
        let suspicious = judgement.score >= threshold;
        let action = match (mode, suspicious) {
            (Mode::Auto, false) => PlannedAction::Verify,
            _ => PlannedAction::AwaitReview,
        };
        Ok(RunPipelineResult { evidence, judgement, mode, threshold, suspicious, action })
    }
}

/// Applies a run's outcome: updates every tracked mod log message, announces
/// approvals, and stops tracking. The tracking entry is claimed atomically,
/// so concurrent resolvers (two moderators, or a moderator racing the
/// poller) act exactly once: the losers get `Ok(false)`, the same as for a
/// run that was never tracked.
pub async fn resolve_pending(
    ctx: &serenity::Context,
    db: &Db,
    run_id: &str,
    outcome: &RunOutcome,
) -> Result<bool, Error> {
    let key = format!("pending:{}", run_id);
    let Some(raw) = db.claim_task_state(TASK_NAME, &key).await? else {
        return Ok(false);
    };
    let Ok(pending) = serde_json::from_str::<PendingRun>(&raw) else {
        return Ok(false);
    };

    let (colour, verdict) = outcome_verdict(outcome);
    update_mod_messages(ctx, &pending.messages, colour, &verdict).await;

    if let Some(review) = &pending.review {
        archive_thread(ctx, review, &format!("Thread closed — {}.", verdict)).await;
    }

    if matches!(outcome, RunOutcome::Approved { .. }) {
        let channels = announce_channels_for(db, &pending.game).await?;
        announce_run(ctx, db, run_id, &pending.game, &channels, false).await?;
    }

    Ok(true)
}

/// Demo counterpart of [`resolve_pending`]: same message updates and
/// announcements, marked as demo, and never any speedrun.com action. Also
/// resolves dry-run pending entries, whose buttons are demo-flavoured.
pub async fn resolve_demo(
    ctx: &serenity::Context,
    db: &Db,
    run_id: &str,
    outcome: &RunOutcome,
) -> Result<bool, Error> {
    let raw = match db.claim_task_state(TASK_NAME, &format!("demo:{}", run_id)).await? {
        Some(raw) => raw,
        None => match db.claim_task_state(TASK_NAME, &format!("pending:{}", run_id)).await? {
            Some(raw) => raw,
            None => return Ok(false),
        },
    };
    let Ok(pending) = serde_json::from_str::<PendingRun>(&raw) else {
        return Ok(false);
    };

    let (colour, mut verdict) = outcome_verdict(outcome);
    verdict.push_str(" — 🧪 demo, no speedrun.com action");
    update_mod_messages(ctx, &pending.messages, colour, &verdict).await;

    if let Some(review) = &pending.review {
        archive_thread(ctx, review, &format!("Thread closed — {}.", verdict)).await;
    }

    if matches!(outcome, RunOutcome::Approved { .. }) {
        let channels = if pending.announce_channels.is_empty() {
            announce_channels_for(db, &pending.game).await?
        } else {
            pending.announce_channels.iter().map(|c| ChannelId::new(*c)).collect()
        };
        if let Some(announcement) = &pending.demo_announcement {
            let embed = announcement_embed(
                &announcement.game_name,
                &announcement.category,
                &announcement.time,
                &announcement.players,
                &announcement.weblink,
                announcement.comment.as_deref(),
                &announcement.videos,
                &announcement.blurbs,
                true,
            );
            for channel in &channels {
                if let Err(e) = channel.send_message(&ctx.http, CreateMessage::new().embed(embed.clone())).await {
                    warn!("Speedrun monitor: announcing to channel {} failed: {:?}", channel, e);
                }
            }
        } else {
            announce_run(ctx, db, run_id, &pending.game, &channels, true).await?;
        }
    }

    Ok(true)
}

/// Outcome of an [`enter_review`] attempt, reported back to the moderator.
pub enum ReviewResult {
    /// A review thread was opened; the value is its id for linking.
    Opened(u64),
    /// The run is already under review; the value is the existing thread id.
    AlreadyOpen(u64),
    /// The run isn't tracked (e.g. already resolved, or never posted).
    NotTracked,
    /// The mod log message has no thread-capable channel, or thread creation
    /// failed. The run is left unchanged.
    Failed,
}

/// Puts a tracked pending run into "pending review": opens a discussion
/// thread off its first mod log message, posts Approve/Reject/Dismiss buttons
/// there, and restamps the mod log embeds. The run stays in the speedrun.com
/// queue — review is a Discord-only state. Idempotent: a run already under
/// review returns its existing thread.
pub async fn enter_review(
    ctx: &serenity::Context,
    db: &Db,
    run_id: &str,
    opened_by: &str,
    demo: bool,
) -> Result<ReviewResult, Error> {
    let key = format!("{}:{}", if demo { "demo" } else { "pending" }, run_id);
    let Some(raw) = db.get_task_state(TASK_NAME, &key).await? else {
        return Ok(ReviewResult::NotTracked);
    };
    let Ok(mut pending) = serde_json::from_str::<PendingRun>(&raw) else {
        return Ok(ReviewResult::NotTracked);
    };

    if let Some(review) = &pending.review {
        return Ok(ReviewResult::AlreadyOpen(review.thread_id));
    }

    // Anchor the thread on the first mod log message. Without one there's
    // nothing to discuss against, so review can't open.
    let Some(&(channel_id, message_id)) = pending.messages.first() else {
        return Ok(ReviewResult::Failed);
    };
    let channel = ChannelId::new(channel_id);
    let message_id = MessageId::new(message_id);

    // Capture the current Status note and colour so a dismiss can restore the
    // embed exactly as it was, rather than re-deriving it from policy.
    let (prior_status, prior_colour) = match channel.message(&ctx.http, message_id).await {
        Ok(message) => {
            let embed = message.embeds.first();
            let status = embed
                .and_then(|e| e.fields.iter().find(|f| f.name == "Status"))
                .map(|f| f.value.clone())
                .unwrap_or_else(|| "⏳ Pending review".to_string());
            let colour = embed.and_then(|e| e.colour).map(|c| c.0).unwrap_or(COLOUR_PENDING);
            (status, colour)
        }
        Err(e) => {
            warn!("Speedrun review: fetching mod log message {} failed: {:?}", message_id, e);
            ("⏳ Pending review".to_string(), COLOUR_PENDING)
        }
    };

    let thread = match channel
        .create_thread_from_message(
            &ctx.http,
            message_id,
            CreateThread::new(format!("Review: run {}", run_id))
                .auto_archive_duration(AutoArchiveDuration::OneWeek),
        )
        .await
    {
        Ok(thread) => thread,
        Err(e) => {
            warn!("Speedrun review: creating thread on message {} failed: {:?}", message_id, e);
            return Ok(ReviewResult::Failed);
        }
    };

    let intro = CreateMessage::new()
        .content(format!(
            "🔍 **{} opened a review.** Discuss here, then Approve or Reject — or Dismiss to return the run to the queue.",
            opened_by
        ))
        .components(vec![thread_buttons(run_id, demo)]);
    let thread_message = match thread.send_message(&ctx.http, intro).await {
        Ok(sent) => Some((thread.id.get(), sent.id.get())),
        Err(e) => {
            warn!("Speedrun review: posting buttons to thread {} failed: {:?}", thread.id, e);
            None
        }
    };

    // Restamp every mod log embed as under review, keeping the buttons live so
    // the run can still be decided from the original message.
    let verdict = format!("🔍 Pending review by {}", opened_by);
    restamp_mod_messages(ctx, &pending.messages, COLOUR_REVIEW, &verdict, demo, run_id).await;

    pending.review = Some(ReviewState {
        thread_id: thread.id.get(),
        thread_message,
        opened_by: opened_by.to_string(),
        prior_status,
        prior_colour,
    });
    db.set_task_state(TASK_NAME, &key, &serde_json::to_string(&pending)?).await?;

    Ok(ReviewResult::Opened(thread.id.get()))
}

/// Dismisses an open review: restores the mod log embeds to their pre-review
/// state, leaves the run tracked and queued, and archives the thread. Any
/// reviewer may dismiss. Returns whether a review was actually cleared.
pub async fn dismiss_review(
    ctx: &serenity::Context,
    db: &Db,
    run_id: &str,
    dismissed_by: &str,
    demo: bool,
) -> Result<bool, Error> {
    let key = format!("{}:{}", if demo { "demo" } else { "pending" }, run_id);
    let Some(raw) = db.get_task_state(TASK_NAME, &key).await? else {
        return Ok(false);
    };
    let Ok(mut pending) = serde_json::from_str::<PendingRun>(&raw) else {
        return Ok(false);
    };
    let Some(review) = pending.review.take() else {
        return Ok(false);
    };

    // Restore the embeds exactly as they were before review opened, buttons
    // included, so the run reverts to a normal pending entry.
    restamp_mod_messages(ctx, &pending.messages, review.prior_colour, &review.prior_status, demo, run_id)
        .await;

    db.set_task_state(TASK_NAME, &key, &serde_json::to_string(&pending)?).await?;

    archive_thread(
        ctx,
        &review,
        &format!("🚪 Review dismissed by {} — run returned to the queue.", dismissed_by),
    )
    .await;

    Ok(true)
}

/// Restamps mod log embeds (colour + Status field) while keeping the review
/// buttons live, so a run under review can still be approved or rejected from
/// the original message.
async fn restamp_mod_messages(
    ctx: &serenity::Context,
    messages: &[(u64, u64)],
    colour: u32,
    verdict: &str,
    demo: bool,
    run_id: &str,
) {
    for (channel_id, message_id) in messages {
        let channel = ChannelId::new(*channel_id);
        let message_id = MessageId::new(*message_id);
        match channel.message(&ctx.http, message_id).await {
            Ok(message) => {
                let embed = verdict_embed(message.embeds.first(), colour, verdict);
                let edit = EditMessage::new()
                    .embed(embed)
                    .components(vec![review_buttons(run_id, demo)]);
                if let Err(e) = channel.edit_message(&ctx.http, message_id, edit).await {
                    warn!("Speedrun review: restamping mod log message {} failed: {:?}", message_id, e);
                }
            }
            Err(e) => warn!("Speedrun review: fetching mod log message {} failed: {:?}", message_id, e),
        }
    }
}

/// Posts a closing note to a review thread, strips its buttons, and archives
/// and locks it. History is retained; the thread just leaves the active list.
async fn archive_thread(ctx: &serenity::Context, review: &ReviewState, note: &str) {
    let thread = ChannelId::new(review.thread_id);

    if let Some((_, message_id)) = review.thread_message {
        let edit = EditMessage::new().components(vec![]);
        if let Err(e) = thread.edit_message(&ctx.http, MessageId::new(message_id), edit).await {
            warn!("Speedrun review: stripping thread buttons in {} failed: {:?}", thread, e);
        }
    }

    if let Err(e) = thread.send_message(&ctx.http, CreateMessage::new().content(note)).await {
        warn!("Speedrun review: posting closing note to thread {} failed: {:?}", thread, e);
    }

    if let Err(e) = thread
        .edit_thread(&ctx.http, EditThread::new().archived(true).locked(true))
        .await
    {
        warn!("Speedrun review: archiving thread {} failed: {:?}", thread, e);
    }
}

async fn update_mod_messages(
    ctx: &serenity::Context,
    messages: &[(u64, u64)],
    colour: u32,
    verdict: &str,
) {
    for (channel_id, message_id) in messages {
        let channel = ChannelId::new(*channel_id);
        let message_id = MessageId::new(*message_id);
        match channel.message(&ctx.http, message_id).await {
            Ok(message) => {
                let embed = verdict_embed(message.embeds.first(), colour, verdict);
                let edit = EditMessage::new().embed(embed).components(vec![]);
                if let Err(e) = channel.edit_message(&ctx.http, message_id, edit).await {
                    warn!("Speedrun monitor: updating mod log message {} failed: {:?}", message_id, e);
                }
            }
            Err(e) => warn!("Speedrun monitor: fetching mod log message {} failed: {:?}", message_id, e),
        }
    }
}

/// Fetches a run's details and announces its approval.
async fn announce_run(
    ctx: &serenity::Context,
    db: &Db,
    run_id: &str,
    abbreviation: &str,
    channels: &[ChannelId],
    demo: bool,
) -> Result<(), Error> {
    match speedrun::get_run(run_id).await {
        Ok(Some(run)) => {
            let game_name = db
                .get_task_state(TASK_NAME, &format!("game_name:{}", abbreviation))
                .await?
                .unwrap_or_else(|| abbreviation.to_string());
            let game_id = db
                .get_task_state(TASK_NAME, &format!("game_id:{}", abbreviation))
                .await?
                .unwrap_or_default();
            announce_approved(ctx, &game_name, &game_id, &run, channels, demo).await;
        }
        Ok(None) => warn!("Speedrun monitor: approved run {} not found for announcement", run_id),
        Err(e) => warn!("Speedrun monitor: fetching run {} for announcement failed: {:?}", run_id, e),
    }
    Ok(())
}

/// Builds the verdict embed used when a mod log message is resolved: the
/// original embed recoloured, with the Status field replaced.
pub fn verdict_embed(original: Option<&Embed>, colour: u32, verdict: &str) -> CreateEmbed {
    let mut embed = CreateEmbed::new();
    if let Some(original) = original {
        if let Some(title) = &original.title {
            embed = embed.title(title.clone());
        }
        if let Some(url) = &original.url {
            embed = embed.url(url.clone());
        }
        if let Some(description) = &original.description {
            embed = embed.description(description.clone());
        }
        for field in original.fields.iter().filter(|f| f.name != "Status") {
            embed = embed.field(field.name.clone(), field.value.clone(), field.inline);
        }
    }
    embed.colour(colour).field("Status", verdict, false)
}

pub fn outcome_verdict(outcome: &RunOutcome) -> (u32, String) {
    match outcome {
        RunOutcome::Approved { by: Some(user) } => (COLOUR_APPROVED, format!("✅ Approved by {}", user)),
        RunOutcome::Approved { by: None } => (COLOUR_APPROVED, "✅ Approved on speedrun.com".to_string()),
        RunOutcome::Rejected { by, reason } => {
            let mut verdict = match by {
                Some(user) => format!("❌ Rejected by {}", user),
                None => "❌ Rejected on speedrun.com".to_string(),
            };
            if let Some(reason) = reason.as_deref().filter(|r| !r.is_empty()) {
                verdict.push_str(&format!(": {}", reason));
            }
            (COLOUR_REJECTED, verdict)
        }
        RunOutcome::Removed => (COLOUR_REMOVED, "🗑️ Removed from the queue on speedrun.com".to_string()),
    }
}

async fn load_policy(db: &Db) -> Result<Policy, Error> {
    let modes = parse_modes(db.get_global_setting(SCOPE, "modes").await?.as_deref().unwrap_or(""));
    let thresholds =
        parse_thresholds(db.get_global_setting(SCOPE, "thresholds").await?.as_deref().unwrap_or(""));
    let default_threshold = match db.get_global_setting(SCOPE, "threshold").await? {
        Some(value) => value.parse().unwrap_or(DEFAULT_THRESHOLD),
        None => DEFAULT_THRESHOLD,
    };
    let dry_run = db
        .get_global_setting(SCOPE, "dry_run")
        .await?
        .is_some_and(|value| matches!(value.as_str(), "true" | "1" | "yes" | "on"));
    Ok(Policy { modes, thresholds, default_threshold, dry_run })
}

/// Parses `game[/category]:value` lists shared by `modes` and `thresholds`.
fn parse_overrides<T>(value: &str, parse: impl Fn(&str) -> Option<T>, what: &str) -> HashMap<String, T> {
    let mut overrides = HashMap::new();
    for entry in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match entry
            .rsplit_once(':')
            .and_then(|(key, value)| parse(value.trim()).map(|v| (key.trim().to_lowercase(), v)))
        {
            Some((key, value)) => {
                overrides.insert(key, value);
            }
            None => warn!("Speedrun monitor: invalid {} entry '{}'", what, entry),
        }
    }
    overrides
}

fn parse_modes(value: &str) -> HashMap<String, Mode> {
    parse_overrides(value, Mode::parse, "modes")
}

fn parse_thresholds(value: &str) -> HashMap<String, u32> {
    parse_overrides(value, |v| v.parse().ok(), "thresholds")
}

fn parse_channel(value: Option<String>) -> Option<ChannelId> {
    value.and_then(|v| v.trim().parse::<u64>().ok()).map(ChannelId::new)
}

async fn post_to_channels(
    ctx: &serenity::Context,
    channels: &[ChannelId],
    embed: &CreateEmbed,
    buttons: Option<CreateActionRow>,
) -> Vec<(u64, u64)> {
    let mut posted = Vec::new();
    for channel in channels {
        let mut message = CreateMessage::new().embed(embed.clone());
        if let Some(buttons) = &buttons {
            message = message.components(vec![buttons.clone()]);
        }
        // One server's missing permissions shouldn't block the others.
        match channel.send_message(&ctx.http, message).await {
            Ok(sent) => posted.push((channel.get(), sent.id.get())),
            Err(e) => warn!("Speedrun monitor: posting to channel {} failed: {:?}", channel, e),
        }
    }
    posted
}

async fn announce_approved(
    ctx: &serenity::Context,
    game_name: &str,
    game_id: &str,
    run: &Run,
    channels: &[ChannelId],
    demo: bool,
) {
    if channels.is_empty() {
        return;
    }
    let blurbs = first_run_blurbs(game_name, game_id, run).await;
    let embed = build_announcement_embed(game_name, run, &blurbs, demo);
    for channel in channels {
        if let Err(e) = channel.send_message(&ctx.http, CreateMessage::new().embed(embed.clone())).await {
            warn!("Speedrun monitor: announcing to channel {} failed: {:?}", channel, e);
        }
    }
}

/// Celebratory notes for players whose approved run is their first verified
/// run in the game, or their first in the category. Best-effort: lookup
/// failures just skip the blurb.
async fn first_run_blurbs(game_name: &str, game_id: &str, run: &Run) -> Vec<String> {
    let mut blurbs = Vec::new();
    if game_id.is_empty() {
        return blurbs;
    }
    for player in &run.players.data {
        let Some(user_id) = &player.id else { continue };
        let name = player.display_name();
        match speedrun::count_verified_runs(user_id, game_id).await {
            // The approved run itself is already counted.
            Ok(count) if count <= 1 => {
                blurbs.push(format!(
                    "🎉 This is {}'s first verified {} run — welcome to the leaderboard!",
                    name, game_name
                ));
                continue;
            }
            Ok(_) => {}
            Err(e) => {
                warn!("Speedrun monitor: game history lookup for '{}' failed: {:?}", name, e);
                continue;
            }
        }
        match speedrun::count_verified_category_runs(user_id, game_id, &run.category.data.id).await {
            Ok(count) if count <= 1 => {
                blurbs.push(format!("✨ {}'s first verified {} run!", name, run.category.data.name));
            }
            Ok(_) => {}
            Err(e) => warn!("Speedrun monitor: category history lookup for '{}' failed: {:?}", name, e),
        }
    }
    blurbs
}

/// Buttons on a mod log message: Approve, Reject, and Review (open a
/// discussion thread).
fn review_buttons(run_id: &str, demo: bool) -> CreateActionRow {
    let (verify, reject, review) = if demo {
        ("speedrun_demo_verify", "speedrun_demo_reject", "speedrun_demo_review")
    } else {
        ("speedrun_verify", "speedrun_reject", "speedrun_review")
    };
    CreateActionRow::Buttons(vec![
        CreateButton::new(format!("{}:{}", verify, run_id))
            .label("Approve")
            .style(ButtonStyle::Success),
        CreateButton::new(format!("{}:{}", reject, run_id))
            .label("Reject")
            .style(ButtonStyle::Danger),
        CreateButton::new(format!("{}:{}", review, run_id))
            .label("Review")
            .style(ButtonStyle::Secondary),
    ])
}

/// Buttons posted inside a review thread: Approve and Reject resolve the run
/// as usual; Dismiss backs out of the review, returning the run to the queue.
fn thread_buttons(run_id: &str, demo: bool) -> CreateActionRow {
    let (verify, reject, dismiss) = if demo {
        ("speedrun_demo_verify", "speedrun_demo_reject", "speedrun_demo_dismiss")
    } else {
        ("speedrun_verify", "speedrun_reject", "speedrun_dismiss")
    };
    CreateActionRow::Buttons(vec![
        CreateButton::new(format!("{}:{}", verify, run_id))
            .label("Approve")
            .style(ButtonStyle::Success),
        CreateButton::new(format!("{}:{}", reject, run_id))
            .label("Reject")
            .style(ButtonStyle::Danger),
        CreateButton::new(format!("{}:{}", dismiss, run_id))
            .label("Dismiss")
            .style(ButtonStyle::Secondary),
    ])
}

/// Announce channels of every server watching this game.
async fn announce_channels_for(db: &Db, abbreviation: &str) -> Result<Vec<ChannelId>, Error> {
    let mut channels = Vec::new();
    for (guild_id, games) in db.guild_setting_values(SCOPE, "games").await? {
        let watching = games
            .split(',')
            .map(str::trim)
            .any(|g| g.eq_ignore_ascii_case(abbreviation));
        if !watching {
            continue;
        }
        if let Some(channel) = parse_channel(db.get_guild_setting(guild_id, SCOPE, "announce_channel").await?) {
            channels.push(channel);
        }
    }
    Ok(channels)
}

fn build_announcement_embed(game_name: &str, run: &Run, blurbs: &[String], demo: bool) -> CreateEmbed {
    let videos: Vec<String> = run.video_links().iter().map(|v| v.to_string()).collect();
    announcement_embed(
        game_name,
        &run.category.data.name,
        &run.formatted_time(),
        &run.player_names(),
        run.weblink.as_str(),
        run.comment.as_deref(),
        &videos,
        blurbs,
        demo,
    )
}

#[allow(clippy::too_many_arguments)]
fn announcement_embed(
    game_name: &str,
    category: &str,
    time: &str,
    players: &str,
    weblink: &str,
    comment: Option<&str>,
    videos: &[String],
    blurbs: &[String],
    demo: bool,
) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title(format!("New {} run: {} in {} by {}", game_name, category, time, players))
        .url(weblink)
        .colour(COLOUR_APPROVED);

    let mut description = blurbs.join("\n");
    if let Some(comment) = comment.map(str::trim).filter(|c| !c.is_empty()) {
        if !description.is_empty() {
            description.push_str("\n\n");
        }
        let comment: String = comment.chars().take(500).collect();
        description.push_str(&format!("> {}", comment.replace('\n', "\n> ")));
    }
    if !description.is_empty() {
        embed = embed.description(description);
    }

    if !videos.is_empty() {
        embed = embed.field("Video", videos.join("\n"), false);
    }

    if demo {
        embed = embed.footer(CreateEmbedFooter::new("🧪 Demo — no action was taken on speedrun.com"));
    }

    embed
}

fn build_mod_embed(
    game: &GameContext<'_>,
    run: &Run,
    result: &RunPipelineResult,
    colour: u32,
    status_note: &str,
) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title(format!(
            "{} submission: {} in {} by {}",
            game.game_name,
            run.category.data.name,
            run.formatted_time(),
            run.player_names()
        ))
        .url(run.weblink.as_str());

    // Mode/threshold details only matter when the bot may act on its own.
    let mut description = format!("**Submitted:** {}", run.submitted.as_deref().unwrap_or("unknown"));
    if result.mode == Mode::Auto {
        description.push_str(&format!(
            "\n**Mode:** {} | **Threshold:** {}",
            result.mode.label(),
            result.threshold
        ));
    }
    embed = embed.description(description);

    if let Some(comment) = run.comment.as_deref().filter(|c| !c.is_empty()) {
        let comment: String = comment.chars().take(500).collect();
        embed = embed.field("Comment", comment, false);
    }

    if !result.evidence.videos.is_empty() {
        // Show the resolved title/channel so reviewers see what the link
        // actually is, not just what the submission claims.
        let videos = result
            .evidence
            .videos
            .iter()
            .map(|v| match (&v.title, &v.channel) {
                (Some(title), Some(channel)) => format!("[{}]({}) — {}", title, v.url, channel),
                (Some(title), None) => format!("[{}]({})", title, v.url),
                _ => v.url.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        embed = embed.field("Video", videos, false);
    }

    let assessment = if result.judgement.reasons.is_empty() {
        "No signals".to_string()
    } else {
        result.judgement.reasons.join("\n")
    };
    embed = embed.field(
        format!("Assessment — score {}/100", result.judgement.score),
        assessment,
        false,
    );

    embed.colour(colour).field("Status", status_note, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            modes: parse_modes("supermetroid:auto, supermetroid/100%:manual"),
            thresholds: parse_thresholds("supermetroid:60, supermetroid/100%:30"),
            default_threshold: 50,
            dry_run: false,
        }
    }

    #[test]
    fn category_override_beats_game_override() {
        let p = policy();
        assert_eq!(p.mode_for("supermetroid", "100%"), Mode::Manual);
        assert_eq!(p.mode_for("supermetroid", "Any%"), Mode::Auto);
        assert_eq!(p.threshold_for("supermetroid", "100%"), 30);
        assert_eq!(p.threshold_for("supermetroid", "Any%"), 60);
    }

    #[test]
    fn unconfigured_game_uses_defaults() {
        let p = policy();
        assert_eq!(p.mode_for("smz3", "Normal"), Mode::Manual);
        assert_eq!(p.threshold_for("smz3", "Normal"), 50);
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let p = policy();
        assert_eq!(p.mode_for("SuperMetroid", "100%"), Mode::Manual);
    }

    #[test]
    fn invalid_entries_are_skipped() {
        let modes = parse_modes("supermetroid:auto,broken,smz3:bogus");
        assert_eq!(modes.len(), 1);
        assert_eq!(modes.get("supermetroid"), Some(&Mode::Auto));
    }
}
