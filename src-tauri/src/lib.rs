pub mod config;
pub mod local_scanner;
pub mod parser;
pub mod registry_client;
pub mod storage_scanner;
pub mod task_engine;

use config::AppConfig;
use local_scanner::LocalPackage;
use parser::ParsedDependency;
use registry_client::SearchResult;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use task_engine::{CacheTask, TaskEngine, TaskStatus};
use tokio::sync::Mutex;

struct AppState {
    task_engine: Arc<Mutex<Option<TaskEngine>>>,
}

#[tauri::command]
fn get_config(app_handle: tauri::AppHandle) -> AppConfig {
    config::load_config(&app_handle)
}

#[tauri::command]
fn save_config(app_handle: tauri::AppHandle, config: AppConfig) -> Result<(), String> {
    config::save_config(&app_handle, &config)
}

#[tauri::command]
async fn test_connection(registry_url: String) -> Result<(), String> {
    let client = registry_client::RegistryClient::new(&registry_url);
    client.test_connection().await
}

#[tauri::command]
async fn search_packages(
    registry_url: String,
    query: String,
) -> Result<Vec<SearchResult>, String> {
    let client = registry_client::RegistryClient::new(&registry_url);
    client.search(&query).await
}

#[tauri::command]
async fn list_cached_via_plugin(registry_url: String) -> Result<Vec<SearchResult>, String> {
    let client = registry_client::RegistryClient::new(&registry_url);
    client.list_cached_via_plugin().await
}

#[tauri::command]
async fn scan_verdaccio_storage(storage_path: String) -> Result<Vec<SearchResult>, String> {
    let path = PathBuf::from(&storage_path);
    storage_scanner::scan_storage(&path)
}

#[tauri::command]
async fn get_package_versions(
    registry_url: String,
    package_name: String,
) -> Result<Vec<String>, String> {
    let client = registry_client::RegistryClient::new(&registry_url);
    client.get_package_versions(&package_name).await
}

#[tauri::command]
async fn get_cached_versions(
    registry_url: String,
    package_name: String,
) -> Result<Vec<String>, String> {
    let client = registry_client::RegistryClient::new(&registry_url);
    match client.get_package_versions(&package_name).await {
        Ok(versions) => Ok(versions),
        Err(_) => Ok(vec![]),
    }
}

#[derive(Deserialize)]
pub struct CacheRequest {
    pub package_name: String,
    pub version: String,
    pub tarball_url: Option<String>,
}

#[tauri::command]
async fn start_cache_tasks(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    packages: Vec<CacheRequest>,
) -> Result<(), String> {
    let config = config::load_config(&app_handle);

    let engine = TaskEngine::new(config.concurrency, config.retry_count, config.timeout_secs);

    for (i, pkg) in packages.iter().enumerate() {
        let task = CacheTask {
            id: format!("task-{}-{}", chrono_id(), i),
            package_name: pkg.package_name.clone(),
            version: pkg.version.clone(),
            tarball_url: pkg.tarball_url.clone(),
            status: TaskStatus::Pending,
            error: None,
        };
        engine.add_task(task).await;
    }

    let source_registry = "https://registry.npmjs.org".to_string();
    let target_registry = config.registry_url.clone();

    {
        let mut lock = state.task_engine.lock().await;
        *lock = Some(engine);
    }

    let engine_ref = state.task_engine.clone();
    let app = app_handle.clone();

    tokio::spawn(async move {
        let engine = engine_ref.lock().await;
        if let Some(ref eng) = *engine {
            eng.execute_all(app, source_registry, target_registry).await;
        }
    });

    Ok(())
}

#[tauri::command]
async fn get_tasks(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CacheTask>, String> {
    let lock = state.task_engine.lock().await;
    if let Some(ref engine) = *lock {
        Ok(engine.get_tasks().await)
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
async fn retry_failed_tasks(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let config = config::load_config(&app_handle);
    let engine_ref = state.task_engine.clone();

    {
        let lock = engine_ref.lock().await;
        if let Some(ref engine) = *lock {
            engine.retry_failed().await;
        }
    }

    let source_registry = "https://registry.npmjs.org".to_string();
    let target_registry = config.registry_url.clone();
    let app = app_handle.clone();

    tokio::spawn(async move {
        let lock = engine_ref.lock().await;
        if let Some(ref engine) = *lock {
            engine.execute_all(app, source_registry, target_registry).await;
        }
    });

    Ok(())
}

#[tauri::command]
async fn clear_completed_tasks(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let lock = state.task_engine.lock().await;
    if let Some(ref engine) = *lock {
        engine.clear_completed().await;
    }
    Ok(())
}

fn chrono_id() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[tauri::command]
async fn parse_file(file_path: String) -> Result<Vec<ParsedDependency>, String> {
    let path = PathBuf::from(&file_path);
    parser::detect_and_parse(&path)
}

#[tauri::command]
async fn scan_node_modules(dir_path: String) -> Result<Vec<LocalPackage>, String> {
    let path = PathBuf::from(&dir_path);
    local_scanner::scan_node_modules(&path)
}

#[tauri::command]
async fn parse_tgz(file_path: String) -> Result<LocalPackage, String> {
    let path = PathBuf::from(&file_path);
    local_scanner::parse_tgz_metadata(&path)
}

#[tauri::command]
async fn upload_tgz_files(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    file_paths: Vec<String>,
) -> Result<(), String> {
    let config = config::load_config(&app_handle);
    let engine = TaskEngine::new(config.concurrency, config.retry_count, config.timeout_secs);

    for (i, fp) in file_paths.iter().enumerate() {
        let path = PathBuf::from(fp);
        let pkg = local_scanner::parse_tgz_metadata(&path)?;
        let task = CacheTask {
            id: format!("upload-{}-{}", chrono_id(), i),
            package_name: pkg.name,
            version: pkg.version,
            tarball_url: Some(format!("file://{}", fp)),
            status: TaskStatus::Pending,
            error: None,
        };
        engine.add_task(task).await;
    }

    let target_registry = config.registry_url.clone();

    {
        let mut lock = state.task_engine.lock().await;
        *lock = Some(engine);
    }

    let engine_ref = state.task_engine.clone();
    let app = app_handle.clone();

    tokio::spawn(async move {
        let lock = engine_ref.lock().await;
        if let Some(ref eng) = *lock {
            eng.execute_all(app, "file://local".to_string(), target_registry)
                .await;
        }
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            task_engine: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            test_connection,
            search_packages,
            list_cached_via_plugin,
            scan_verdaccio_storage,
            get_package_versions,
            get_cached_versions,
            start_cache_tasks,
            get_tasks,
            retry_failed_tasks,
            clear_completed_tasks,
            parse_file,
            scan_node_modules,
            parse_tgz,
            upload_tgz_files,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
