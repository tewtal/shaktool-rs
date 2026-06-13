use crate::tasks::speedrun::{
    enter_review, DemoResult, Mode, ReviewResult, SpeedrunDebugReport, SpeedrunDebugResult,
    SpeedrunMonitor,
};
use crate::{Context, Error};

/// Speedrun.com moderation tools
#[poise::command(
    prefix_command,
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    subcommands("debug", "demo", "showcase", "review")
)]
pub async fn speedrun(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Available subcommands: `debug`, `demo`, `showcase`, `review`").await?;
    Ok(())
}

/// Open a discussion thread on a tracked run and set it pending review
#[poise::command(prefix_command, slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn review(
    ctx: Context<'_>,
    #[description = "speedrun.com run id or run URL"] run: String,
) -> Result<(), Error> {
    let Some(run_id) = parse_run_id(&run) else {
        ctx.say("Couldn't read a run id from that. Pass a run id or a speedrun.com/run/<id> URL.")
            .await?;
        return Ok(());
    };

    ctx.defer_ephemeral().await?;

    let result = enter_review(
        ctx.serenity_context(),
        &ctx.data().db,
        &run_id,
        &ctx.author().name,
        false,
    )
    .await?;

    let message = match result {
        ReviewResult::Opened(thread_id) => format!("🔍 Review opened: <#{}>", thread_id),
        ReviewResult::AlreadyOpen(thread_id) => {
            format!("This run is already under review: <#{}>", thread_id)
        }
        ReviewResult::NotTracked => format!(
            "Run `{}` isn't in the mod log yet. Wait for the monitor to post it, then use the Review button.",
            run_id
        ),
        ReviewResult::Failed => {
            "Couldn't open a review thread — the mod log channel may not support threads.".to_string()
        }
    };
    ctx.say(message).await?;
    Ok(())
}

/// Accepts a bare run id or a speedrun.com run URL, returning the id.
fn parse_run_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    // URLs look like https://www.speedrun.com/run/<id> or .../runs/<id>.
    let id = match trimmed.rsplit_once("/run/").or_else(|| trimmed.rsplit_once("/runs/")) {
        Some((_, tail)) => tail,
        None => trimmed,
    };
    // Strip any query string or trailing slash, and reject anything that isn't
    // a plain id token.
    let id = id.split(['/', '?', '#']).next().unwrap_or(id).trim();
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(id.to_string())
    } else {
        None
    }
}

/// Post fake scenario submissions showing flags, scores and announcements
#[poise::command(prefix_command, slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn showcase(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or_default();
    let monitor = SpeedrunMonitor::new();
    let result = monitor
        .demo_showcase(ctx.serenity_context(), &ctx.data().db, guild_id)
        .await?;

    match result {
        DemoResult::Posted(count) => {
            ctx.say(format!(
                "Posted {} fake showcase scenarios to the mod log. Everything is invented — approve one with first-run blurbs to see the announcement. Nothing is sent to speedrun.com.",
                count
            ))
            .await?
        }
        _ => {
            ctx.say("No mod log channel configured. Set one with `config set speedrun mod_channel <channel id>`.")
                .await?
        }
    };
    Ok(())
}

/// Post recent submissions to the mod log as a demo; nothing is sent to speedrun.com
#[poise::command(prefix_command, slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn demo(
    ctx: Context<'_>,
    #[description = "speedrun.com game abbreviation"] game: String,
    #[description = "Number of recent submissions to post (default 3, max 10)"] count: Option<usize>,
) -> Result<(), Error> {
    let count = count.unwrap_or(3).clamp(1, 10);
    ctx.defer().await?;

    let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or_default();
    let monitor = SpeedrunMonitor::new();
    let result = monitor
        .demo_recent_runs(ctx.serenity_context(), &ctx.data().db, guild_id, &game, count)
        .await?;

    match result {
        DemoResult::Posted(count) => {
            ctx.say(format!(
                "Posted {} demo submission(s) to the mod log. The buttons update messages and announce approvals, but nothing is sent to speedrun.com.",
                count
            ))
            .await?
        }
        DemoResult::NoModChannel => {
            ctx.say("No mod log channel configured. Set one with `config set speedrun mod_channel <channel id>`.")
                .await?
        }
        DemoResult::UnknownGame => {
            ctx.say(format!("No speedrun.com game found for abbreviation `{}`.", game)).await?
        }
    };
    Ok(())
}

/// Dry-run recent submissions through the moderation pipeline
#[poise::command(prefix_command, slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn debug(
    ctx: Context<'_>,
    #[description = "speedrun.com game abbreviation"] game: String,
    #[description = "Number of recent submissions to test (default 5, max 200)"] count: Option<usize>,
    #[description = "Optional mode override: manual or auto"] mode: Option<String>,
) -> Result<(), Error> {
    let count = count.unwrap_or(5).clamp(1, 200);
    let mode = match mode.as_deref() {
        Some(mode) => match Mode::parse(&mode.to_ascii_lowercase()) {
            Some(mode) => Some(mode),
            None => {
                ctx.say("Invalid mode. Use `manual` or `auto`.").await?;
                return Ok(());
            }
        },
        None => None,
    };

    ctx.defer().await?;

    let monitor = SpeedrunMonitor::new();
    let Some(report) = monitor.debug_recent_runs(&ctx.data().db, &game, count, mode).await? else {
        ctx.say(format!("No speedrun.com game found for abbreviation `{}`.", game)).await?;
        return Ok(());
    };

    for message in format_report(&report) {
        ctx.say(message).await?;
    }

    Ok(())
}

fn format_report(report: &SpeedrunDebugReport) -> Vec<String> {
    let mut messages = Vec::new();
    let mut current = format!(
        "**Speedrun moderation dry run**\nGame: **{}** (`{}`)\nDefault mode: `{}` | Default threshold: `{}` | Requested runs: `{}` | Returned runs: `{}`\n",
        report.game_name,
        report.abbreviation,
        report.default_mode.label(),
        report.default_threshold,
        report.requested_count,
        report.results.len()
    );

    if report.results.is_empty() {
        current.push_str("\nNo recent submissions returned by speedrun.com.");
        return vec![current];
    }

    for result in &report.results {
        let block = format_result(result);
        if current.len() + block.len() > 1900 {
            messages.push(current);
            current = String::new();
        }
        current.push_str(&block);
    }

    if !current.is_empty() {
        messages.push(current);
    }
    messages
}

fn format_result(result: &SpeedrunDebugResult) -> String {
    let mut output = format!(
        "\n<{}>\n`{}` | `{}` | `{}` | {} by {}\n",
        result.weblink, result.run_id, result.status, result.submitted, result.time, result.players
    );
    output.push_str(&format!("Category: `{}`\n", result.category));

    if let Some(skipped) = &result.skipped {
        output.push_str(&format!("Skipped: {}\n", skipped));
        return output;
    }

    let score = result.score.map_or_else(|| "n/a".to_string(), |score| score.to_string());
    let suspicious = match result.suspicious {
        Some(true) => "yes",
        Some(false) => "no",
        None => "n/a",
    };
    let mode = result.mode.unwrap_or("n/a");
    let threshold = result.threshold.map_or_else(|| "n/a".to_string(), |t| t.to_string());
    output.push_str(&format!(
        "Mode: `{}` | Threshold: `{}` | Score: `{}/100` | Suspicious: `{}`\nAction: {}\n",
        mode, threshold, score, suspicious, result.action
    ));

    if result.reasons.is_empty() {
        output.push_str("Reasons: none\n");
    } else {
        let reasons = result
            .reasons
            .iter()
            .take(5)
            .map(|reason| format!("- {}", truncate(reason, 240)))
            .collect::<Vec<_>>()
            .join("\n");
        output.push_str("Reasons:\n");
        output.push_str(&reasons);
        if result.reasons.len() > 5 {
            output.push_str(&format!("\n- ...and {} more", result.reasons.len() - 5));
        }
        output.push('\n');
    }

    output
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut output: String = value.chars().take(max_chars).collect();
    if output.len() < value.len() {
        output.push_str("...");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::parse_run_id;

    #[test]
    fn parses_bare_id() {
        assert_eq!(parse_run_id("zpwl3krm").as_deref(), Some("zpwl3krm"));
        assert_eq!(parse_run_id("  zpwl3krm  ").as_deref(), Some("zpwl3krm"));
    }

    #[test]
    fn parses_run_urls() {
        assert_eq!(
            parse_run_id("https://www.speedrun.com/run/zpwl3krm").as_deref(),
            Some("zpwl3krm")
        );
        assert_eq!(
            parse_run_id("https://www.speedrun.com/runs/zpwl3krm/").as_deref(),
            Some("zpwl3krm")
        );
        assert_eq!(
            parse_run_id("https://www.speedrun.com/run/zpwl3krm?foo=bar").as_deref(),
            Some("zpwl3krm")
        );
    }

    #[test]
    fn rejects_non_id_input() {
        assert_eq!(parse_run_id(""), None);
        assert_eq!(parse_run_id("   "), None);
        // A bare weblink with no id segment isn't a usable run id.
        assert_eq!(parse_run_id("https://www.speedrun.com/supermetroid"), None);
    }
}
