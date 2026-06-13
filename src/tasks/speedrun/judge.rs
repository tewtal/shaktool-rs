//! Evidence gathering and suspicion scoring for speedrun.com submissions.
//!
//! [`gather`] collects everything knowable about a run into a serializable
//! [`Evidence`] value, and a [`Judge`] turns that into a [`Judgement`] with a
//! 0-100 suspicion score. [`RuleJudge`] is the default; an LLM-backed judge
//! can implement the same trait over the serialized evidence (it derives
//! `Serialize` for exactly that purpose) without any changes to the monitor
//! task.

use async_trait::async_trait;
use serde::Serialize;
use tracing::warn;

use crate::api::oembed;
use crate::api::speedrun::{self, Run};
use crate::Error;

/// Video hosts where speedrun footage is normally published. Links anywhere
/// else are treated as potentially malicious.
const KNOWN_HOSTS: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "twitch.tv",
    "vimeo.com",
    "nicovideo.jp",
];

/// Verified runs in the game after which a player counts as established.
const ESTABLISHED_RUNS: usize = 3;

#[derive(Serialize, Debug)]
pub struct Evidence {
    pub game: String,
    pub abbreviation: String,
    pub category: String,
    pub time_seconds: f64,
    pub comment: Option<String>,
    pub videos: Vec<VideoEvidence>,
    pub players: Vec<PlayerEvidence>,
    /// Top leaderboard times (seconds) for the category, best first.
    pub top_times: Vec<f64>,
}

#[derive(Serialize, Debug)]
pub struct VideoEvidence {
    pub url: String,
    pub host: String,
    pub known_host: bool,
    pub title: Option<String>,
    pub channel: Option<String>,
    /// The host confirmed the video does not exist (deleted or private).
    pub unavailable: bool,
}

#[derive(Serialize, Debug)]
pub struct PlayerEvidence {
    pub name: String,
    pub guest: bool,
    /// None when unknown (guest account or lookup failed).
    pub verified_runs_in_game: Option<usize>,
}

#[derive(Debug)]
pub struct Judgement {
    /// 0 (no concerns) to 100 (almost certainly bogus).
    pub score: u32,
    pub reasons: Vec<String>,
}

#[async_trait]
pub trait Judge: Send + Sync {
    async fn judge(&self, evidence: &Evidence) -> Result<Judgement, Error>;
}

/// Collects evidence about a run. Lookups are best-effort: failures are
/// logged and leave the corresponding evidence empty rather than aborting,
/// so a flaky external service degrades the judgement instead of blocking it.
pub async fn gather(abbreviation: &str, game_name: &str, game_id: &str, run: &Run) -> Evidence {
    let mut videos = Vec::new();
    for url in run.video_links() {
        videos.push(video_evidence(url).await);
    }

    let mut players = Vec::new();
    for player in &run.players.data {
        let verified_runs_in_game = match &player.id {
            Some(id) => match speedrun::count_verified_runs(id, game_id).await {
                Ok(count) => Some(count),
                Err(e) => {
                    warn!("Speedrun judge: history lookup for '{}' failed: {:?}", player.display_name(), e);
                    None
                }
            },
            None => None,
        };
        players.push(PlayerEvidence {
            name: player.display_name().to_string(),
            guest: player.is_guest(),
            verified_runs_in_game,
        });
    }

    let top_times = match speedrun::get_top_times(game_id, &run.category.data.id, 3).await {
        Ok(times) => times,
        Err(e) => {
            warn!("Speedrun judge: leaderboard lookup for category '{}' failed: {:?}", run.category.data.name, e);
            Vec::new()
        }
    };

    Evidence {
        game: game_name.to_string(),
        abbreviation: abbreviation.to_string(),
        category: run.category.data.name.clone(),
        time_seconds: run.times.primary_t,
        comment: run.comment.clone(),
        videos,
        players,
        top_times,
    }
}

async fn video_evidence(url: &str) -> VideoEvidence {
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
        .unwrap_or_else(|| "invalid url".to_string());
    let known_host = KNOWN_HOSTS.iter().any(|known| host == *known || host.ends_with(&format!(".{}", known)));

    let mut evidence = VideoEvidence {
        url: url.to_string(),
        host: host.clone(),
        known_host,
        title: None,
        channel: None,
        unavailable: false,
    };

    if host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be" {
        match oembed::youtube_info(url).await {
            Ok(Some(info)) => {
                evidence.title = Some(info.title);
                evidence.channel = info.author_name;
            }
            Ok(None) => evidence.unavailable = true,
            Err(e) => warn!("Speedrun judge: video lookup for <{}> failed: {:?}", url, e),
        }
    }

    evidence
}

/// Rule-based judge: additive suspicion signals, with a large trust discount
/// for established runners so they generally always pass.
pub struct RuleJudge;

#[async_trait]
impl Judge for RuleJudge {
    async fn judge(&self, evidence: &Evidence) -> Result<Judgement, Error> {
        let mut score: i32 = 0;
        let mut reasons: Vec<String> = Vec::new();

        if evidence.videos.is_empty() {
            score += 40;
            reasons.push("No video attached".to_string());
        }
        for video in &evidence.videos {
            if !video.known_host {
                score += 35;
                reasons.push(format!("Video hosted on unrecognized site: {}", video.host));
            }
            if video.unavailable {
                score += 25;
                reasons.push(format!("Video is unavailable or deleted: <{}>", video.url));
            }
            if let Some(title) = &video.title {
                if !title_seems_related(title, evidence) {
                    score += 10;
                    reasons.push(format!("Video title doesn't obviously match the run: \"{}\"", title));
                }
            }
        }

        if evidence.time_seconds < 1.0 {
            score += 50;
            reasons.push("Submitted time is under one second".to_string());
        }

        let established = evidence
            .players
            .iter()
            .any(|p| p.verified_runs_in_game.unwrap_or(0) >= ESTABLISHED_RUNS);

        if !established {
            if let Some(&slowest_top) = evidence.top_times.last() {
                if evidence.time_seconds <= slowest_top {
                    score += 30;
                    reasons.push(format!(
                        "Would be a top-{} {} time, from a runner with little history",
                        evidence.top_times.len(),
                        evidence.category
                    ));
                }
            }
        }

        for player in &evidence.players {
            if player.guest {
                score += 15;
                reasons.push(format!("{} is a guest (anonymous) submitter", player.name));
            } else {
                match player.verified_runs_in_game {
                    Some(0) => {
                        score += 25;
                        reasons.push(format!("First submission to this game by {}", player.name));
                    }
                    Some(count @ 1..=2) => {
                        score += 10;
                        reasons.push(format!("{} has only {} verified run(s) in this game", player.name, count));
                    }
                    _ => {}
                }
            }
        }

        if established {
            score -= 60;
            reasons.push(format!("Established runner ({}+ verified runs in this game)", ESTABLISHED_RUNS));
        }

        Ok(Judgement { score: score.clamp(0, 100) as u32, reasons })
    }
}

/// Generous relevance check for video titles, to keep false positives down:
/// any digit (times, percentages) or any word from the game/category/
/// abbreviation counts as related.
fn title_seems_related(title: &str, evidence: &Evidence) -> bool {
    let title = title.to_lowercase();
    if title.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if title.contains(&evidence.abbreviation.to_lowercase()) {
        return true;
    }
    evidence
        .game
        .split_whitespace()
        .chain(evidence.category.split_whitespace())
        .filter(|word| word.len() >= 4)
        .any(|word| title.contains(&word.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> Evidence {
        Evidence {
            game: "Super Metroid".to_string(),
            abbreviation: "supermetroid".to_string(),
            category: "Any%".to_string(),
            time_seconds: 2500.0,
            comment: None,
            videos: vec![],
            players: vec![],
            top_times: vec![2400.0, 2450.0, 2480.0],
        }
    }

    fn video(host: &str, title: Option<&str>) -> VideoEvidence {
        VideoEvidence {
            url: format!("https://{}/abc", host),
            host: host.to_string(),
            known_host: KNOWN_HOSTS.contains(&host),
            title: title.map(str::to_string),
            channel: None,
            unavailable: false,
        }
    }

    fn player(name: &str, verified_runs: Option<usize>) -> PlayerEvidence {
        PlayerEvidence {
            name: name.to_string(),
            guest: verified_runs.is_none(),
            verified_runs_in_game: verified_runs,
        }
    }

    async fn score(evidence: &Evidence) -> u32 {
        RuleJudge.judge(evidence).await.unwrap().score
    }

    #[tokio::test]
    async fn established_runner_passes_without_video() {
        let mut e = evidence();
        e.players = vec![player("veteran", Some(42))];
        assert!(score(&e).await < 50);
    }

    #[tokio::test]
    async fn new_runner_without_video_is_flagged() {
        let mut e = evidence();
        e.players = vec![player("newcomer", Some(0))];
        assert!(score(&e).await >= 50);
    }

    #[tokio::test]
    async fn competitive_time_from_unknown_runner_is_flagged() {
        let mut e = evidence();
        e.time_seconds = 2300.0;
        e.players = vec![player("newcomer", Some(0))];
        e.videos = vec![video("youtube.com", Some("Super Metroid Any% in 38:20"))];
        assert!(score(&e).await >= 50);
    }

    #[tokio::test]
    async fn competitive_time_from_established_runner_passes() {
        let mut e = evidence();
        e.time_seconds = 2300.0;
        e.players = vec![player("veteran", Some(42))];
        e.videos = vec![video("youtube.com", Some("Super Metroid Any% in 38:20"))];
        assert_eq!(score(&e).await, 0);
    }

    #[tokio::test]
    async fn unknown_video_host_is_flagged_for_new_runner() {
        let mut e = evidence();
        e.players = vec![player("newcomer", Some(0))];
        e.videos = vec![video("totally-not-a-virus.example", None)];
        assert!(score(&e).await >= 50);
    }

    #[tokio::test]
    async fn unrelated_video_title_adds_suspicion_but_is_not_decisive() {
        let mut e = evidence();
        e.time_seconds = 3000.0;
        e.players = vec![player("casual", Some(2))];
        e.videos = vec![video("youtube.com", Some("My cool vacation vlog"))];
        let judgement = RuleJudge.judge(&e).await.unwrap();
        assert!(judgement.reasons.iter().any(|r| r.contains("doesn't obviously match")));
        assert!(judgement.score < 50);
    }
}
