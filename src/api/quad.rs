use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::error::Error as StdError;

type ApiResult<T> = Result<T, Box<dyn StdError + Send + Sync>>;

pub const DEFAULT_BASE_URL: &str = "https://quad.samus.link";
const DEFAULT_LANGUAGE: &str = "en";
const DEFAULT_GAME: &str = "Combo";

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct RandomizerRequest {
    pub seed: u64,
    pub include_spoiler: bool,
    pub configs: Vec<Map<String, Value>>,
    #[serde(skip)]
    pub base_url: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct RandomizerResponse {
    pub id: String,
    pub seed: i64,
    pub worlds: Map<String, Value>,
    #[serde(rename = "spoilerLog")]
    pub spoiler_log: Option<Value>,
}

impl RandomizerResponse {
    pub fn permalink(&self, base_url: &str) -> String {
        format!("{}/seed/{}", base_url.trim_end_matches('/'), self.id)
    }
}

impl RandomizerRequest {
    pub fn quad() -> Self {
        let mut world = Map::new();
        world.insert("Language".to_string(), json!(DEFAULT_LANGUAGE));
        world.insert("Game".to_string(), json!(DEFAULT_GAME));
        world.insert("Alttp".to_string(), json!({}));
        world.insert("Zelda1".to_string(), json!({}));
        world.insert("SuperMetroid".to_string(), json!({}));
        world.insert("Metroid".to_string(), json!({}));
        world.insert("Combo".to_string(), json!({}));

        Self {
            seed: 0,
            include_spoiler: true,
            configs: vec![world],
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    pub fn set_game_enabled(&mut self, game: &str, enabled: bool) {
        let world = self.world_config_mut();
        if enabled {
            world.entry(game.to_string()).or_insert_with(|| json!({}));
        } else {
            world.remove(game);
        }
    }

    pub fn set_world_option(&mut self, key: &str, value: Value) {
        self.world_config_mut().insert(key.to_string(), value);
    }

    pub fn set_game_option(&mut self, game: &str, key: &str, value: Value) {
        let world = self.world_config_mut();
        let entry = world.entry(game.to_string()).or_insert_with(|| json!({}));
        if !entry.is_object() {
            *entry = json!({});
        }
        if let Some(options) = entry.as_object_mut() {
            options.insert(key.to_string(), value);
        }
    }

    pub fn set_base_url(&mut self, base_url: &str) {
        self.base_url = base_url.trim_end_matches('/').to_string();
    }

    pub async fn send(&self) -> ApiResult<RandomizerResponse> {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/api/randomize", self.base_url))
            .json(self)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(format!("csrando API returned {}: {}", status, body).into());
        }

        Ok(serde_json::from_str(&body)?)
    }

    fn world_config_mut(&mut self) -> &mut Map<String, Value> {
        if self.configs.is_empty() {
            self.configs.push(Map::new());
        }
        &mut self.configs[0]
    }
}

pub async fn metadata(base_url: &str) -> ApiResult<Value> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/api/metadata/Combo", base_url.trim_end_matches('/')))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("csrando metadata API returned {}: {}", status, body).into());
    }

    Ok(serde_json::from_str(&body)?)
}
