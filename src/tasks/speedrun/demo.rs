//! Demo and showcase tooling for the speedrun monitor.
//!
//! Everything here exists to exercise and visualise the moderation flow
//! without touching speedrun.com: `demo_recent_runs` replays real recent
//! submissions through the pipeline with simulated buttons, and
//! `demo_showcase` posts entirely fabricated scenarios covering the
//! interesting evidence cases. None of it runs in production; it's all
//! reached through the admin-only `speedrun` command.

use std::collections::HashMap;

use poise::serenity_prelude as serenity;
use tracing::warn;

use super::judge::{Evidence, PlayerEvidence, VideoEvidence};
use super::{
    build_mod_embed, load_policy, parse_channel, pending_colour, post_to_channels, review_buttons,
    DemoAnnouncement, DemoResult, GameContext, Mode, PendingRun, PlannedAction, Policy,
    RunPipelineResult, SpeedrunMonitor, DEFAULT_THRESHOLD, SCOPE, TASK_NAME,
};
use crate::api::speedrun::{self, Category, Embedded, Names, Player, Run, RunStatus, Times, VideoLink, Videos};
use crate::db::Db;
use crate::Error;

/// Status note shown on demo/showcase mod log messages, where the buttons are
/// always simulated.
fn demo_status_note(action: PlannedAction, mode: Mode) -> &'static str {
    match (action, mode) {
        (PlannedAction::Verify, _) => {
            "🧪 Demo — auto mode would approve this run; buttons simulate the flow"
        }
        (PlannedAction::AwaitReview, Mode::Auto) => {
            "🧪 Demo — would be flagged for manual review; buttons simulate the flow"
        }
        (PlannedAction::AwaitReview, Mode::Manual) => {
            "🧪 Demo — pending manual review; buttons simulate the flow"
        }
    }
}

/// Posts the latest submissions (any status) to one server's channels as a
/// demo of the moderation flow. The buttons work — messages update and
/// approvals are announced — but nothing is ever sent to speedrun.com.
pub async fn demo_recent_runs(
    monitor: &SpeedrunMonitor,
    ctx: &serenity::Context,
    db: &Db,
    guild_id: u64,
    abbreviation: &str,
    count: usize,
) -> Result<DemoResult, Error> {
    let Some(mod_channel) = parse_channel(db.get_guild_setting(guild_id, SCOPE, "mod_channel").await?)
    else {
        return Ok(DemoResult::NoModChannel);
    };
    let announce_channel =
        parse_channel(db.get_guild_setting(guild_id, SCOPE, "announce_channel").await?);

    let policy = load_policy(db).await?;
    let Some((game_id, game_name)) = monitor.resolve_game(db, abbreviation).await? else {
        return Ok(DemoResult::UnknownGame);
    };
    let game = GameContext {
        abbreviation,
        game_name,
        game_id,
        mod_channels: vec![mod_channel],
        announce_channels: announce_channel.into_iter().collect(),
        api_key: None,
        policy: &policy,
    };

    let runs = speedrun::get_runs_limited(&game.game_id, None, count.clamp(1, 10)).await?;
    let mut posted = 0;
    for run in runs.iter().rev() {
        let result = monitor.evaluate_run(&game, run, None).await?;
        let status_note = demo_status_note(result.action, result.mode);
        let colour = pending_colour(result.mode, result.suspicious);
        let embed = build_mod_embed(&game, run, &result, colour, status_note);
        let messages =
            post_to_channels(ctx, &game.mod_channels, &embed, Some(review_buttons(&run.id, true))).await;
        if messages.is_empty() {
            continue;
        }
        posted += 1;
        let pending = PendingRun {
            game: abbreviation.to_string(),
            messages,
            announce_channels: game.announce_channels.iter().map(|c| c.get()).collect(),
            demo_announcement: None,
            review: None,
        };
        db.set_task_state(TASK_NAME, &format!("demo:{}", run.id), &serde_json::to_string(&pending)?)
            .await?;
    }

    Ok(DemoResult::Posted(posted))
}

/// Posts entirely fabricated submissions to one server's mod log, showcasing
/// the different flags, scores, and (on approval) the first-run announcement
/// blurbs. The fake runs go through the real judge and embeds; only the
/// evidence is invented.
pub async fn demo_showcase(
    monitor: &SpeedrunMonitor,
    ctx: &serenity::Context,
    db: &Db,
    guild_id: u64,
) -> Result<DemoResult, Error> {
    let Some(mod_channel) = parse_channel(db.get_guild_setting(guild_id, SCOPE, "mod_channel").await?)
    else {
        return Ok(DemoResult::NoModChannel);
    };
    let announce_channel =
        parse_channel(db.get_guild_setting(guild_id, SCOPE, "announce_channel").await?);

    let game_name = "Super Demotroid";
    let policy = Policy {
        modes: HashMap::new(),
        thresholds: HashMap::new(),
        default_threshold: DEFAULT_THRESHOLD,
        dry_run: true,
    };
    let game = GameContext {
        abbreviation: "superdemotroid",
        game_name: game_name.to_string(),
        game_id: String::new(),
        mod_channels: vec![mod_channel],
        announce_channels: announce_channel.into_iter().collect(),
        api_key: None,
        policy: &policy,
    };

    // Unique ids per invocation so repeated showcases don't collide.
    let stamp = chrono::Utc::now().timestamp_millis();
    let mut posted = 0;
    for (index, scenario) in showcase_scenarios(game_name).into_iter().enumerate() {
        let mut run = scenario.run;
        run.id = format!("showcase-{}-{}", stamp, index);

        let judgement = monitor.judge.judge(&scenario.evidence).await?;
        let suspicious = judgement.score >= policy.default_threshold;
        let action = match (scenario.mode, suspicious) {
            (Mode::Auto, false) => PlannedAction::Verify,
            _ => PlannedAction::AwaitReview,
        };
        let result = RunPipelineResult {
            evidence: scenario.evidence,
            judgement,
            mode: scenario.mode,
            threshold: policy.default_threshold,
            suspicious,
            action,
        };
        let status_note = demo_status_note(action, scenario.mode);
        let colour = pending_colour(result.mode, result.suspicious);

        let embed = build_mod_embed(&game, &run, &result, colour, status_note);
        let message = serenity::CreateMessage::new()
            .content(format!("🧪 **Scenario:** {}", scenario.label))
            .embed(embed)
            .components(vec![review_buttons(&run.id, true)]);
        match mod_channel.send_message(&ctx.http, message).await {
            Ok(sent) => {
                posted += 1;
                let announcement = DemoAnnouncement {
                    game_name: game_name.to_string(),
                    category: run.category.data.name.clone(),
                    time: run.formatted_time(),
                    players: run.player_names(),
                    weblink: run.weblink.clone(),
                    comment: run.comment.clone(),
                    videos: run.video_links().iter().map(|v| v.to_string()).collect(),
                    blurbs: scenario.blurbs,
                };
                let pending = PendingRun {
                    game: game.abbreviation.to_string(),
                    messages: vec![(mod_channel.get(), sent.id.get())],
                    announce_channels: game.announce_channels.iter().map(|c| c.get()).collect(),
                    demo_announcement: Some(announcement),
                    review: None,
                };
                db.set_task_state(TASK_NAME, &format!("demo:{}", run.id), &serde_json::to_string(&pending)?)
                    .await?;
            }
            Err(e) => warn!("Speedrun monitor: posting showcase scenario failed: {:?}", e),
        }
    }

    Ok(DemoResult::Posted(posted))
}

struct ShowcaseScenario {
    label: &'static str,
    mode: Mode,
    run: Run,
    evidence: Evidence,
    blurbs: Vec<String>,
}

/// Fabricated submissions covering the interesting cases: each scenario's
/// evidence is scored by the real judge, so the scores shown are genuine.
fn showcase_scenarios(game_name: &str) -> Vec<ShowcaseScenario> {
    let top_times = vec![2450.0, 2500.0, 2550.0];

    vec![
        ShowcaseScenario {
            label: "Clean run from an established runner",
            mode: Mode::Manual,
            run: fake_run("Any%", 2710.0, Some("PB! Finally sub-46."), Some("https://youtu.be/demo-clean"), Some("PixelPete")),
            evidence: fake_evidence(
                game_name,
                "Any%",
                2710.0,
                vec![fake_video("https://youtu.be/demo-clean", "youtu.be", true, Some("Super Demotroid Any% in 45:10"), Some("PixelPete"), false)],
                vec![fake_player("PixelPete", false, Some(42))],
                top_times.clone(),
            ),
            blurbs: vec![],
        },
        ShowcaseScenario {
            label: "No video from a first-time submitter",
            mode: Mode::Auto,
            run: fake_run("Any%", 2950.0, None, None, Some("FreshFace42")),
            evidence: fake_evidence(
                game_name,
                "Any%",
                2950.0,
                vec![],
                vec![fake_player("FreshFace42", false, Some(0))],
                top_times.clone(),
            ),
            blurbs: vec![format!(
                "🎉 This is FreshFace42's first verified {} run — welcome to the leaderboard!",
                game_name
            )],
        },
        ShowcaseScenario {
            label: "Video hosted on an unrecognized (possibly malicious) site",
            mode: Mode::Auto,
            run: fake_run("100%", 5400.0, Some("free game cheats at the link!!"), Some("https://totally-not-a-virus.example/video"), Some("ShadyLad")),
            evidence: fake_evidence(
                game_name,
                "100%",
                5400.0,
                vec![fake_video("https://totally-not-a-virus.example/video", "totally-not-a-virus.example", false, None, None, false)],
                vec![fake_player("ShadyLad", false, Some(0))],
                vec![4800.0, 4900.0, 5000.0],
            ),
            blurbs: vec![],
        },
        ShowcaseScenario {
            label: "Video deleted or made private",
            mode: Mode::Auto,
            run: fake_run("Any%", 2800.0, None, Some("https://youtu.be/gone-forever"), Some("VanishingAct")),
            evidence: fake_evidence(
                game_name,
                "Any%",
                2800.0,
                vec![fake_video("https://youtu.be/gone-forever", "youtu.be", true, None, None, true)],
                vec![fake_player("VanishingAct", false, Some(0))],
                top_times.clone(),
            ),
            blurbs: vec![],
        },
        ShowcaseScenario {
            label: "Would-be world record from a runner with no history",
            mode: Mode::Auto,
            run: fake_run("Any%", 2400.0, Some("got it first try :)"), Some("https://youtu.be/demo-wr"), Some("NewWorldRecord")),
            evidence: fake_evidence(
                game_name,
                "Any%",
                2400.0,
                vec![fake_video("https://youtu.be/demo-wr", "youtu.be", true, Some("Super Demotroid Any% 40:00 WR?"), Some("NewWorldRecord"), false)],
                vec![fake_player("NewWorldRecord", false, Some(0))],
                top_times.clone(),
            ),
            blurbs: vec![],
        },
        ShowcaseScenario {
            label: "Sub-second time from an anonymous guest",
            mode: Mode::Auto,
            run: fake_run("Any%", 0.42, Some("EZ"), None, None),
            evidence: fake_evidence(
                game_name,
                "Any%",
                0.42,
                vec![],
                vec![fake_player("Anonymous", true, None)],
                top_times.clone(),
            ),
            blurbs: vec![],
        },
        ShowcaseScenario {
            label: "Veteran's first run in a new category",
            mode: Mode::Manual,
            run: fake_run("100%", 6100.0, Some("First 100% attempt, loved the route!"), Some("https://youtu.be/demo-cat"), Some("CategoryCurious")),
            evidence: fake_evidence(
                game_name,
                "100%",
                6100.0,
                vec![fake_video("https://youtu.be/demo-cat", "youtu.be", true, Some("Super Demotroid 100% in 1:41:40"), Some("CategoryCurious"), false)],
                vec![fake_player("CategoryCurious", false, Some(12))],
                vec![4800.0, 4900.0, 5000.0],
            ),
            blurbs: vec!["✨ CategoryCurious's first verified 100% run!".to_string()],
        },
    ]
}

fn fake_run(
    category: &str,
    seconds: f64,
    comment: Option<&str>,
    video_url: Option<&str>,
    player: Option<&str>,
) -> Run {
    Run {
        id: String::new(),
        weblink: "https://www.speedrun.com/".to_string(),
        comment: comment.map(str::to_string),
        submitted: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        status: RunStatus { status: "new".to_string(), reason: None },
        times: Times { primary_t: seconds },
        videos: video_url.map(|url| Videos { links: Some(vec![VideoLink { uri: url.to_string() }]) }),
        players: Embedded {
            data: vec![match player {
                Some(name) => Player {
                    rel: Some("user".to_string()),
                    id: Some("demo".to_string()),
                    names: Some(Names { international: name.to_string() }),
                    name: None,
                },
                None => Player {
                    rel: Some("guest".to_string()),
                    id: None,
                    names: None,
                    name: Some("Anonymous".to_string()),
                },
            }],
        },
        category: Embedded { data: Category { id: "demo".to_string(), name: category.to_string() } },
    }
}

fn fake_evidence(
    game_name: &str,
    category: &str,
    seconds: f64,
    videos: Vec<VideoEvidence>,
    players: Vec<PlayerEvidence>,
    top_times: Vec<f64>,
) -> Evidence {
    Evidence {
        game: game_name.to_string(),
        abbreviation: "superdemotroid".to_string(),
        category: category.to_string(),
        time_seconds: seconds,
        comment: None,
        videos,
        players,
        top_times,
    }
}

fn fake_video(
    url: &str,
    host: &str,
    known_host: bool,
    title: Option<&str>,
    channel: Option<&str>,
    unavailable: bool,
) -> VideoEvidence {
    VideoEvidence {
        url: url.to_string(),
        host: host.to_string(),
        known_host,
        title: title.map(str::to_string),
        channel: channel.map(str::to_string),
        unavailable,
    }
}

fn fake_player(name: &str, guest: bool, verified_runs_in_game: Option<usize>) -> PlayerEvidence {
    PlayerEvidence { name: name.to_string(), guest, verified_runs_in_game }
}
