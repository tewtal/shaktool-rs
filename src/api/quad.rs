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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub configs: Vec<Map<String, Value>>,
    #[serde(skip)]
    pub base_url: String,
    #[serde(skip)]
    pub api_key: Option<String>,
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

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PresetSummary {
    pub id: String,
    pub slug: Option<String>,
    pub name: String,
    #[serde(default)]
    pub selected_games: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct PresetsResponse {
    #[serde(default)]
    pub officials: Vec<PresetSummary>,
    #[serde(default)]
    pub mine: Vec<PresetSummary>,
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
            preset_id: None,
            revision_id: None,
            configs: vec![world],
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: None,
        }
    }

    pub fn set_preset(&mut self, preset_id: &str, revision_id: Option<&str>) {
        self.preset_id = Some(preset_id.to_string());
        self.revision_id = revision_id.map(str::to_string);
        self.configs.clear();
    }

    pub fn is_preset(&self) -> bool {
        self.preset_id.is_some()
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

    pub fn set_api_key(&mut self, api_key: Option<&str>) {
        self.api_key = api_key.map(str::to_string);
    }

    pub async fn send(&self) -> ApiResult<RandomizerResponse> {
        let client = reqwest::Client::new();
        let mut request = client
            .post(format!("{}/api/randomize", self.base_url))
            .json(self);
        if let Some(api_key) = self.api_key.as_deref() {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await?;

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

pub async fn presets(base_url: &str, api_key: Option<&str>) -> ApiResult<PresetsResponse> {
    let client = reqwest::Client::new();
    let mut request = client.get(format!(
        "{}/api/presets?configId=combo",
        base_url.trim_end_matches('/')
    ));
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }
    let response = request.send().await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("csrando presets API returned {}: {}", status, body).into());
    }

    Ok(serde_json::from_str(&body)?)
}

pub async fn metadata(base_url: &str) -> ApiResult<Value> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "{}/api/metadata/Combo",
            base_url.trim_end_matches('/')
        ))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("csrando metadata API returned {}: {}", status, body).into());
    }

    Ok(serde_json::from_str(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_request_serializes_configs_without_preset_fields_or_secrets() {
        let mut request = RandomizerRequest::quad();
        request.set_api_key(Some("qr_secret"));

        let value = serde_json::to_value(&request).unwrap();
        assert!(value.get("Configs").is_some());
        assert!(value.get("PresetId").is_none());
        assert!(value.get("RevisionId").is_none());
        assert!(!value.to_string().contains("qr_secret"));
    }

    #[test]
    fn preset_request_uses_preset_contract_without_configs() {
        let mut request = RandomizerRequest::quad();
        request.seed = 123;
        request.include_spoiler = false;
        request.set_preset("preset-id", Some("revision-id"));

        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "Seed": 123,
                "IncludeSpoiler": false,
                "PresetId": "preset-id",
                "RevisionId": "revision-id"
            })
        );
    }

    #[test]
    fn preset_list_deserializes_camel_case_selected_games() {
        let presets: PresetsResponse = serde_json::from_value(json!({
            "officials": [{
                "id": "id",
                "scope": "official",
                "slug": "recommended",
                "name": "Recommended",
                "selectedGames": ["Alttpr", "Sm"]
            }],
            "mine": []
        }))
        .unwrap();

        assert_eq!(presets.officials[0].selected_games, vec!["Alttpr", "Sm"]);
    }
}
