use reqwest::header::USER_AGENT;
use serde::Deserialize;

use crate::Error;

const AGENT: &str = "shaktool-rs/2.0";

#[derive(Deserialize, Debug)]
pub struct VideoInfo {
    pub title: String,
    pub author_name: Option<String>,
}

/// Fetches title and channel for a YouTube video via the keyless oEmbed
/// endpoint. `Ok(None)` means YouTube reports the video as unavailable
/// (deleted, private or never existed).
pub async fn youtube_info(video_url: &str) -> Result<Option<VideoInfo>, Error> {
    let url = format!(
        "https://www.youtube.com/oembed?url={}&format=json",
        urlencoding::encode(video_url)
    );
    let response = reqwest::Client::new()
        .get(&url)
        .header(USER_AGENT, AGENT)
        .send()
        .await?;
    if response.status().is_client_error() {
        return Ok(None);
    }
    let info: VideoInfo = response.error_for_status()?.json().await?;
    Ok(Some(info))
}
