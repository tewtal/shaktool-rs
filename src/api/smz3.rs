use reqwest;
use serde_json;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use strum_macros::EnumString;
use tracing::{error, info, debug};
use crate::util::slugid;
type ApiError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Deserialize, Debug)]
pub struct Randomizer {
    pub id: String,
    pub name: String,
    pub version: String,
    pub options: Vec<RandomizerOption>,    
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RandomizerOptionType {
    Dropdown,
    Seed,
    Checkbox,
    Players,    
}

#[derive(Deserialize, Debug)]
pub struct RandomizerOption {
    pub key: String,
    pub description: String,
    #[serde(rename = "type")]
    pub option_type: RandomizerOptionType,
    pub values: Option<HashMap<String, String>>,
    pub default: Option<String>
}

#[derive(EnumString, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum SMLogic {
    Normal,
    Hard,
}

#[derive(EnumString, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum Goal {
    DefeatBoth,
    FastGanonDefeatMotherBrain,
    AllDungeonsDefeatMotherBrain,
}

#[derive(EnumString, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum OpenTower {
    NoCrystals = 0,
    OneCrystal = 1,
    TwoCrystals = 2,
    ThreeCrystals = 3,
    FourCrystals = 4,
    FiveCrystals = 5,
    SixCrystals = 6,
    SevenCrystals = 7,
    Random = 8,
}

impl OpenTower {
    pub fn from_int(i: i64) -> Self {
        match i {
            0 => OpenTower::NoCrystals,
            1 => OpenTower::OneCrystal,
            2 => OpenTower::TwoCrystals,
            3 => OpenTower::ThreeCrystals,
            4 => OpenTower::FourCrystals,
            5 => OpenTower::FiveCrystals,
            6 => OpenTower::SixCrystals,
            7 => OpenTower::SevenCrystals,
            _ => OpenTower::Random,
        }
    }
}

#[derive(EnumString, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum GanonVulnerable {
    NoCrystals = 0,
    OneCrystal = 1,
    TwoCrystals = 2,
    ThreeCrystals = 3,
    FourCrystals = 4,
    FiveCrystals = 5,
    SixCrystals = 6,
    SevenCrystals = 7,
    Random = 8,
}

impl GanonVulnerable {
    pub fn from_int(i: i64) -> Self {
        match i {
            0 => GanonVulnerable::NoCrystals,
            1 => GanonVulnerable::OneCrystal,
            2 => GanonVulnerable::TwoCrystals,
            3 => GanonVulnerable::ThreeCrystals,
            4 => GanonVulnerable::FourCrystals,
            5 => GanonVulnerable::FiveCrystals,
            6 => GanonVulnerable::SixCrystals,
            7 => GanonVulnerable::SevenCrystals,
            _ => GanonVulnerable::Random,
        }
    }
}

#[derive(EnumString, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum OpenTourian {
    NoBosses = 0,
    OneBoss = 1,
    TwoBosses = 2,
    ThreeBosses = 3,
    FourBosses = 4,
    Random = 5,
}

impl OpenTourian {
    pub fn from_int(i: i64) -> Self {
        match i {
            0 => OpenTourian::NoBosses,
            1 => OpenTourian::OneBoss,
            2 => OpenTourian::TwoBosses,
            3 => OpenTourian::ThreeBosses,
            4 => OpenTourian::FourBosses,
            _ => OpenTourian::Random,
        }
    }
}

#[derive(EnumString, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum SwordLocation {
    #[serde(rename = "randomized")]
    Random,
    Early,
    UncleAssured,
}

#[derive(EnumString, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum MorphLocation {
    #[serde(rename = "randomized")]
    Random,
    Early,
    OriginalLocation,
}

#[derive(EnumString, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum KeyShuffle {
    None,
    Keysanity,
}

#[derive(EnumString, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum GameMode {
    Normal,
    Multiworld,
}

#[derive(Serialize, Debug)]
pub struct RandomizerRequest {
    pub gamemode: GameMode,
    pub ganonvulnerable: GanonVulnerable,
    pub goal: Goal,
    pub keyshuffle: KeyShuffle,
    pub morphlocation: MorphLocation,
    pub opentourian: OpenTourian,
    pub opentower: OpenTower,
    pub players: i64,
    pub race: bool,
    pub seed: String,
    pub smlogic: SMLogic,
    pub swordlocation: SwordLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialitems: Option<String>,
    #[serde(skip)]
    pub beta: bool,
    #[serde(skip)]
    pub names: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
pub struct RandomizerResponse {
    pub id: i64,
    pub guid: String,
    pub mode: String,
    #[serde(rename = "seedNumber")]
    pub seed_number: String,
    pub spoiler: String,
    #[serde(rename = "gameName")]
    pub game_name: String,
    #[serde(rename = "gameVersion")]
    pub game_version: String,
    #[serde(rename = "gameId")]
    pub game_id: String,
    pub hash: String,
    pub players: i64,
    pub worlds: Vec<RandomizerWorld>,
    #[serde(skip)]
    pub beta: bool,
}

impl RandomizerResponse {
    pub fn permalink(&self) -> String {
	let seed_link = match self.mode.as_str() {
            "multiworld" => "multiworld".to_string(),
            _ => "seed".to_string()
        };

        if self.beta {
            format!("https://beta.samus.link/{}/{}", seed_link, slugid::create(&self.guid).unwrap())
        } else {
            format!("https://samus.link/{}/{}", seed_link, slugid::create(&self.guid).unwrap())
        }        
    }
}

#[derive(Deserialize, Debug)]
pub struct RandomizerWorld {
    pub id: i64,
    #[serde(rename = "worldId")]
    pub world_id: i64,
    #[serde(rename = "seedId")]
    pub seed_id: i64,
    pub guid: String,
    pub player: String,
    pub settings: String,
    pub state: i64,
    pub patch: String,
    #[serde(rename = "sramBackup")]
    pub sram_backup: Option<String>,
    //pub locations: Option<String>,
    #[serde(rename = "worldState")]
    pub world_state: Option<String>,
}

impl RandomizerRequest {
    pub fn default() -> Self {
        Self {
            gamemode: GameMode::Normal,
            ganonvulnerable: GanonVulnerable::SevenCrystals,
            goal: Goal::DefeatBoth,
            keyshuffle: KeyShuffle::None,
            morphlocation: MorphLocation::Random,
            opentourian: OpenTourian::FourBosses,
            opentower: OpenTower::SevenCrystals,
            players: 1,
            race: false,
            seed: String::from(""),
            smlogic: SMLogic::Normal,
            swordlocation: SwordLocation::Random,
            initialitems: None,
            beta: false,
            names: None,
        }
    }

    pub async fn send(&self) -> Result<RandomizerResponse, ApiError> {
        let client = reqwest::Client::new();
        
        let url = if self.beta {
            "https://beta.samus.link/api/randomizers/smz3/generate"
        } else {
            "https://samus.link/api/randomizers/smz3/generate"
        };

        let mut json_object = serde_json::to_value(&self)?.as_object().unwrap().clone();;

        if self.gamemode == GameMode::Multiworld
        {
            if let Some(names) = &self.names {
                for (i, name) in names.iter().enumerate() {
                    let name_str = format!("player-{}", i);
                    let name_object = serde_json::to_value(&name)?;
                    json_object.insert(name_str, name_object);
                }

                let player_count = format!("{}", names.len());
                json_object["players"] = serde_json::from_str(&player_count)?;
		
            }
        }


        let res = client.post(url)
            .json(&json_object)
            .send()
            .await?;
        let body = res.text().await?;
        let mut response: RandomizerResponse = serde_json::from_str(&body)?;
        response.beta = self.beta;
        Ok(response)
    }

}
