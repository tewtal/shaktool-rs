use reqwest;
use serde_json;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct DeerTierRecord {
    #[serde(rename="ID")]
    pub id: i32,
    #[serde(rename="Username")]
    pub username: String,
    #[serde(rename="Category")]
    pub category: String,
    #[serde(rename="RealTime")]
    pub real_time: Option<String>,
    #[serde(rename="GameTime")]
    pub game_time: Option<String>,
    #[serde(rename="EscapeGameTime")]
    pub escape_game_time: Option<String>,
    #[serde(rename="VideoUrl")]
    pub video_url: Option<String>,
    #[serde(rename="Comment")]
    pub comment: Option<String>,
    #[serde(rename="DateSubmitted")]
    pub date_submitted: Option<String>
}

impl DeerTierRecord {
    pub async fn get_all_records() -> Result<Vec<DeerTierRecord>, Box<dyn std::error::Error + Send + Sync>> {
        let reqclient = reqwest::Client::new();
        let response = reqclient.get("https://deertier.com/api/records").send().await?;
        let body = response.text().await?;
        let records: Vec<DeerTierRecord> = serde_json::from_str(&body)?;
        Ok(records)
    }    
}

