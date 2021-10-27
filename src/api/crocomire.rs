use reqwest;
use serde_json;
use serde::{Deserialize};

#[derive(Deserialize, Debug)]
pub struct Strategy {
    pub area_name: String,
    pub category_name: String,
    pub created_on: String,
    pub description: String,
    pub difficulty: i32,
    pub game_name: String,
    pub id: i32,
    pub link: String,
    pub name: String,
    pub room_name: String,
    pub user_name: String
}

impl Strategy {
    pub async fn find(strat: &str) -> Result<Vec<Strategy>, Box<dyn std::error::Error + Send + Sync>> {
        let reqclient = reqwest::Client::new();
        let response = reqclient.get(format!("https://crocomi.re/api/strats/{}", strat).as_str()).send().await?;
        let body = response.text().await?;
        let data: serde_json::Value = serde_json::from_str(&body)?;
        let strats = data.pointer("/strats").ok_or("Could not find the strategies node in the json data.")?.to_owned();
        let strat_list = serde_json::from_value::<Vec<Strategy>>(strats)?;
        Ok(strat_list)
    }
}