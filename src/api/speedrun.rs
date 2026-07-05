use std::io;
use std::time::Duration;

use reqwest::header::USER_AGENT;
use serde::Deserialize;
use tokio::time::sleep;

use crate::Error;

const API_BASE: &str = "https://www.speedrun.com/api/v1";
const AGENT: &str = "shaktool-rs/2.0";
const STATUS_CONFIRM_ATTEMPTS: usize = 3;
const STATUS_CONFIRM_DELAY: Duration = Duration::from_millis(750);

#[derive(Deserialize, Debug)]
pub struct Embedded<T> {
    pub data: T,
}

#[derive(Deserialize, Debug)]
pub struct Names {
    pub international: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Game {
    pub id: String,
    pub names: Names,
    pub abbreviation: String,
    pub weblink: String,
}

#[derive(Deserialize, Debug)]
pub struct Category {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct Player {
    pub rel: Option<String>,
    pub id: Option<String>,
    pub names: Option<Names>,
    pub name: Option<String>,
}

impl Player {
    pub fn display_name(&self) -> &str {
        self.names
            .as_ref()
            .map(|n| n.international.as_str())
            .or(self.name.as_deref())
            .unwrap_or("Unknown")
    }

    /// Guests are anonymous submitters without a speedrun.com account.
    pub fn is_guest(&self) -> bool {
        self.rel.as_deref() == Some("guest") || self.id.is_none()
    }
}

#[derive(Deserialize, Debug)]
pub struct Times {
    pub primary_t: f64,
}

#[derive(Deserialize, Debug)]
pub struct VideoLink {
    pub uri: String,
}

#[derive(Deserialize, Debug)]
pub struct Videos {
    pub links: Option<Vec<VideoLink>>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct RunStatus {
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Run {
    pub id: String,
    pub weblink: String,
    pub comment: Option<String>,
    pub submitted: Option<String>,
    pub status: RunStatus,
    pub times: Times,
    pub videos: Option<Videos>,
    pub players: Embedded<Vec<Player>>,
    pub category: Embedded<Category>,
}

impl Run {
    pub fn player_names(&self) -> String {
        self.players
            .data
            .iter()
            .map(|p| p.display_name())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn video_links(&self) -> Vec<&str> {
        self.videos
            .as_ref()
            .and_then(|v| v.links.as_ref())
            .map(|links| links.iter().map(|l| l.uri.as_str()).collect())
            .unwrap_or_default()
    }

    pub fn formatted_time(&self) -> String {
        let total = self.times.primary_t;
        let secs = total as u64;
        let millis = ((total - secs as f64) * 1000.0).round() as u64;
        let (hours, minutes, seconds) = (secs / 3600, (secs % 3600) / 60, secs % 60);
        let mut out = if hours > 0 {
            format!("{}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{}:{:02}", minutes, seconds)
        };
        if millis > 0 {
            out.push_str(&format!(".{:03}", millis));
        }
        out
    }
}

/// Looks up a game by its speedrun.com abbreviation (e.g. "sm" or "smz3").
pub async fn get_game(abbreviation: &str) -> Result<Option<Game>, Error> {
    let url = format!(
        "{}/games?abbreviation={}",
        API_BASE,
        urlencoding::encode(abbreviation)
    );
    let response = reqwest::Client::new()
        .get(&url)
        .header(USER_AGENT, AGENT)
        .send()
        .await?
        .error_for_status()?;
    let games: Embedded<Vec<Game>> = response.json().await?;
    Ok(games.data.into_iter().next())
}

/// Fetches the most recently submitted runs for a game, optionally filtered
/// by status ("new", "verified" or "rejected").
pub async fn get_runs(game_id: &str, status: Option<&str>) -> Result<Vec<Run>, Error> {
    get_runs_limited(game_id, status, 50).await
}

/// Fetches up to `max` of the most recently submitted runs for a game.
pub async fn get_runs_limited(
    game_id: &str,
    status: Option<&str>,
    max: usize,
) -> Result<Vec<Run>, Error> {
    let max = max.clamp(1, 200);
    let status_filter = status.map(|s| format!("&status={}", s)).unwrap_or_default();
    let url = format!(
        "{}/runs?game={}{}&orderby=submitted&direction=desc&max={}&embed=players,category",
        API_BASE, game_id, status_filter, max
    );
    let response = reqwest::Client::new()
        .get(&url)
        .header(USER_AGENT, AGENT)
        .send()
        .await?
        .error_for_status()?;
    let runs: Embedded<Vec<Run>> = response.json().await?;
    Ok(runs.data)
}

/// Fetches a single run by id. `Ok(None)` means the run no longer exists
/// (deleted by the runner or moderators).
pub async fn get_run(run_id: &str) -> Result<Option<Run>, Error> {
    let url = format!("{}/runs/{}?embed=players,category", API_BASE, run_id);
    let response = reqwest::Client::new()
        .get(&url)
        .header(USER_AGENT, AGENT)
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let run: Embedded<Run> = response.error_for_status()?.json().await?;
    Ok(Some(run.data))
}

/// Counts the verified runs a user has in a game (capped at 200).
pub async fn count_verified_runs(user_id: &str, game_id: &str) -> Result<usize, Error> {
    let url = format!(
        "{}/runs?user={}&game={}&status=verified&max=200",
        API_BASE, user_id, game_id
    );
    let response = reqwest::Client::new()
        .get(&url)
        .header(USER_AGENT, AGENT)
        .send()
        .await?
        .error_for_status()?;
    let runs: Embedded<Vec<serde_json::Value>> = response.json().await?;
    Ok(runs.data.len())
}

/// Counts the verified runs a user has in one category of a game (capped at 200).
pub async fn count_verified_category_runs(
    user_id: &str,
    game_id: &str,
    category_id: &str,
) -> Result<usize, Error> {
    let url = format!(
        "{}/runs?user={}&game={}&category={}&status=verified&max=200",
        API_BASE, user_id, game_id, category_id
    );
    let response = reqwest::Client::new()
        .get(&url)
        .header(USER_AGENT, AGENT)
        .send()
        .await?
        .error_for_status()?;
    let runs: Embedded<Vec<serde_json::Value>> = response.json().await?;
    Ok(runs.data.len())
}

#[derive(Deserialize, Debug)]
struct Leaderboard {
    runs: Vec<LeaderboardEntry>,
}

#[derive(Deserialize, Debug)]
struct LeaderboardEntry {
    run: LeaderboardRun,
}

#[derive(Deserialize, Debug)]
struct LeaderboardRun {
    times: Times,
}

/// Fetches the top times (in seconds, best first) of a full-game category
/// leaderboard, across all variable values.
pub async fn get_top_times(game_id: &str, category_id: &str, top: u32) -> Result<Vec<f64>, Error> {
    let url = format!(
        "{}/leaderboards/{}/category/{}?top={}",
        API_BASE, game_id, category_id, top
    );
    let response = reqwest::Client::new()
        .get(&url)
        .header(USER_AGENT, AGENT)
        .send()
        .await?
        .error_for_status()?;
    let leaderboard: Embedded<Leaderboard> = response.json().await?;
    Ok(leaderboard
        .data
        .runs
        .into_iter()
        .map(|entry| entry.run.times.primary_t)
        .collect())
}

pub enum RunStatusChange {
    Verified,
    Rejected { reason: String },
}

impl RunStatusChange {
    fn status(&self) -> &'static str {
        match self {
            RunStatusChange::Verified => "verified",
            RunStatusChange::Rejected { .. } => "rejected",
        }
    }
}

/// Changes the verification status of a run. The API key must belong to a
/// moderator of the run's game.
pub async fn set_run_status(
    api_key: &str,
    run_id: &str,
    change: &RunStatusChange,
) -> Result<(), Error> {
    let body = match change {
        RunStatusChange::Verified => serde_json::json!({"status": {"status": "verified"}}),
        RunStatusChange::Rejected { reason } => {
            serde_json::json!({"status": {"status": "rejected", "reason": reason}})
        }
    };
    reqwest::Client::new()
        .put(format!("{}/runs/{}/status", API_BASE, run_id))
        .header(USER_AGENT, AGENT)
        .header("X-API-Key", api_key)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    confirm_status_change(run_id, change).await?;
    Ok(())
}

async fn confirm_status_change(run_id: &str, change: &RunStatusChange) -> Result<(), Error> {
    let expected = change.status();
    let mut last_status = None;

    for attempt in 0..STATUS_CONFIRM_ATTEMPTS {
        match get_run(run_id).await? {
            Some(run) if run.status.status == expected => return Ok(()),
            Some(run) => last_status = Some(run.status.status),
            None => {
                return Err(io::Error::other(format!(
                    "speedrun.com status update for run {} returned success, but the run no longer exists",
                    run_id
                ))
                .into());
            }
        }

        if attempt + 1 < STATUS_CONFIRM_ATTEMPTS {
            sleep(STATUS_CONFIRM_DELAY).await;
        }
    }

    Err(unconfirmed_status_change_error(
        run_id,
        expected,
        last_status.as_deref().unwrap_or("unknown"),
    ))
}

fn unconfirmed_status_change_error(run_id: &str, expected: &str, actual: &str) -> Error {
    io::Error::other(format!(
        "speedrun.com status update for run {} returned success, but follow-up lookup still showed '{}'; expected '{}'",
        run_id, actual, expected
    ))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfirmed_status_change_error_describes_expected_status() {
        let error = unconfirmed_status_change_error("abc123", "verified", "new");

        assert!(error.to_string().contains("expected 'verified'"));
    }

    #[test]
    fn run_status_change_exposes_expected_status() {
        let reject = RunStatusChange::Rejected {
            reason: "bad video".to_string(),
        };

        assert_eq!(RunStatusChange::Verified.status(), "verified");
        assert_eq!(reject.status(), "rejected");
    }

    #[tokio::test]
    #[ignore = "hits the live speedrun.com API"]
    async fn fetch_new_runs() {
        let game = get_game("supermetroid")
            .await
            .unwrap()
            .expect("game not found");
        assert_eq!(game.names.international, "Super Metroid");

        for status in [None, Some("new")] {
            let runs = get_runs(&game.id, status).await.unwrap();
            for run in &runs {
                assert!(!run.id.is_empty());
                assert!(!run.status.status.is_empty());
                assert!(!run.player_names().is_empty());
                assert!(!run.formatted_time().is_empty());
            }
        }

        let runs = get_runs(&game.id, None).await.unwrap();
        if let Some(run) = runs.first() {
            let top = get_top_times(&game.id, &run.category.data.id, 3)
                .await
                .unwrap();
            assert!(top.iter().all(|t| *t > 0.0));

            if let Some(user_id) = run.players.data.iter().find_map(|p| p.id.as_deref()) {
                count_verified_runs(user_id, &game.id).await.unwrap();
            }
        }
    }
}
