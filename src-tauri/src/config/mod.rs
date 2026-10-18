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
    /// 解析依赖时用于匹配 `engines.node` 的目标 Node 版本；为空则使用本机版本。
    #[serde(default)]
    pub target_node_version: Option<String>,
    #[serde(default)]
    pub health_check: HealthCheckConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HealthCheckConfig {
    pub on_startup: bool,
    /// 0 表示关闭定时体检。
    pub interval_minutes: u32,
    pub mode: HealthCheckMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckMode {
    #[default]
    Local,
    Remote,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            registry_url: "http://localhost:4873".to_string(),
            concurrency: 5,
            retry_count: 3,
            timeout_secs: 60,
            verdaccio_storage_path: None,
            target_node_version: None,
            health_check: HealthCheckConfig::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_keeps_existing_values_and_disables_automatic_checks() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "registry_url": "http://offline:4873", "concurrency": 2,
            "retry_count": 1, "timeout_secs": 30
        }))
        .unwrap();
        assert_eq!(config.registry_url, "http://offline:4873");
        assert_eq!(config.health_check, HealthCheckConfig::default());
    }

    #[test]
    fn automatic_check_preferences_round_trip() {
        let config = AppConfig {
            health_check: HealthCheckConfig {
                on_startup: true,
                interval_minutes: 360,
                mode: HealthCheckMode::Remote,
            },
            ..AppConfig::default()
        };
        let saved = serde_json::to_string(&config).unwrap();
        let restored: AppConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.health_check, config.health_check);
    }
}
