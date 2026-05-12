use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub registry_url: String,
    pub concurrency: u32,
    pub retry_count: u32,
    pub timeout_secs: u64,
    #[serde(default)]
    pub verdaccio_storage_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            registry_url: "http://localhost:4873".to_string(),
            concurrency: 5,
            retry_count: 3,
            timeout_secs: 60,
            verdaccio_storage_path: None,
        }
    }
}

pub fn config_path(app_handle: &tauri::AppHandle) -> PathBuf {
    let dir = app_handle
        .path()
        .app_data_dir()
        .expect("failed to get app data dir");
    dir.join("config.json")
}

pub fn load_config(app_handle: &tauri::AppHandle) -> AppConfig {
    let path = config_path(app_handle);
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AppConfig::default()
    }
}

pub fn save_config(app_handle: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let path = config_path(app_handle);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(())
}
