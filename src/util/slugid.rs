use base64::{engine::general_purpose, Engine as _};
use std::error::Error;

pub fn create(uuid: &str) -> Result<String, Box<dyn Error>> {
    let uuid = uuid::Uuid::parse_str(uuid)?;
    let slug = general_purpose::URL_SAFE_NO_PAD.encode(uuid.as_bytes());
    Ok(slug)
}
