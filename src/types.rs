use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(rename = "availableInWinget")]
    pub available_in_winget: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub apps: Vec<AppConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct InstalledApp {
    pub name: String,
    pub id: String,
    pub version: String,
    pub source: String,
}
