use std::error::Error;

pub fn create(uuid: &str) -> Result<String, Box<dyn Error>> {
    let uuid = uuid::Uuid::parse_str(uuid)?;
    Ok(base64::encode_config(uuid.as_bytes(), base64::URL_SAFE_NO_PAD))
}