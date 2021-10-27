use reqwest;
use serde::Deserialize;

type CardError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Deserialize, Debug)]
pub struct Card
{
    pub name: String,
    #[serde(rename="manaCost")]
    pub mana_cost: Option<String>,
    pub cmc: Option<f64>,
    #[serde(rename="type")]
    pub card_type: String,
    pub rarity: String,
    pub set: String,
    pub text: String,
    pub power: Option<String>,
    pub toughness: Option<String>,
    #[serde(rename="imageUrl")]
    pub image_url: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CardResult
{
    pub cards: Vec<Card>
}

pub async fn get_card(name: &str) -> Result<CardResult, CardError>
{
    let response = reqwest::get(format!("https://api.magicthegathering.io/v1/cards?name=\"{}\"", name)).await?;
    let result = response.json::<CardResult>().await?;
    Ok(result)
}