use std::error::Error;
use base64::{Engine as _, engine::general_purpose};

pub fn create(uuid: &str) -> Result<String, Box<dyn Error>> {
    let uuid = uuid::Uuid::parse_str(uuid)?;
    let slug = general_purpose::URL_SAFE_NO_PAD.encode(uuid.as_bytes());
    Ok(slug)
}