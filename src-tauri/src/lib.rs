pub mod cache_db;
pub mod config;
pub mod dependency_resolver;
pub mod local_scanner;
pub mod parser;
pub mod registry_client;
pub mod storage_scanner;
pub mod sync_engine;
pub mod task_engine;

use cache_db::{CacheDb, CachedStatus, SyncInfo};
use config::AppConfig;
use dependency_resolver::ResolvedDep;
use local_scanner::LocalPackage;
use parser::ParsedDependency;
use registry_client::SearchResult;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use sync_engine::SyncEngine;
use tauri::Manager;
use task_engine::{CacheTask, TaskEngine, TaskStatus};
use tokio::sync::Mutex;

struct AppState {
    task_engine: Arc<Mutex<Option<TaskEngine>>>,
    cache_db: Arc<Mutex<CacheDb>>,
    sync_engine: Arc<SyncEngine>,
}

#[tauri::command]
fn get_config(app_handle: tauri::AppHandle) -> AppConfig {
    config::load_config(&app_handle)
}

#[tauri::command]
fn save_config(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<(), String> {
    let old_config = config::load_config(&app_handle);
    config::save_config(&app_handle, &config)?;

    if old_config.registry_url != config.registry_url && !config.registry_url.is_empty() {
        let db = state.cache_db.clone();
        let sync = state.sync_engine.clone();
        let app = app_handle.clone();
        let registry_url = config.registry_url.clone();
        let storage_path = config.verdaccio_storage_path.clone();
        tauri::async_runtime::spawn(async move {
            if !sync.is_running() {
                let db_lock = db.lock().await;
                let _ = db_lock.clear_all();
                drop(db_lock);
                sync.start_sync(app, registry_url, storage_path, db).await;
            }
        });
    }

    Ok(())
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

#[derive(serde::Serialize, Clone)]
struct ResolveProgressEvent {
    request_id: String,
    name: String,
    raw_range: String,
    version: Option<String>,
    cached: bool,
    error: Option<String>,
}

#[derive(serde::Serialize, Clone)]
struct ResolvedImportPackage {
    name: String,
    raw_range: String,
    version: String,
    tarball_url: Option<String>,
    cached: bool,
}

#[tauri::command]
async fn resolve_package_versions(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    packages: Vec<CacheRequest>,
    request_id: String,
) -> Result<Vec<ResolvedImportPackage>, String> {
    use std::collections::HashMap;
    use tauri::Emitter;
    use tokio::sync::Semaphore;

    let http = reqwest::Client::new();
    let sem = Arc::new(Semaphore::new(10));
    let versions_cache: Arc<Mutex<HashMap<String, Arc<Vec<String>>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let db = state.cache_db.clone();

    let mut handles = Vec::with_capacity(packages.len());
    for pkg in packages {
        let http = http.clone();
        let sem = sem.clone();
        let cache = versions_cache.clone();
        let app = app_handle.clone();
        let db = db.clone();
        let request_id = request_id.clone();

        handles.push(tokio::spawn(async move {
            let name = pkg.package_name.clone();
            let raw_range = pkg.version.clone();

            let resolved = if let Some(v) = parser::pinned_version(&raw_range) {
                Some(v)
            } else {
                let versions = {
                    let map = cache.lock().await;
                    map.get(&name).cloned()
                };
                let versions = match versions {
                    Some(v) => Some(v),
                    None => match parser::fetch_versions(&http, &sem, &name).await {
                        Ok(v) => {
                            let arc = Arc::new(v);
                            cache.lock().await.insert(name.clone(), arc.clone());
                            Some(arc)
                        }
                        Err(_) => None,
                    },
                };
                versions.and_then(|vs| parser::resolve_max_satisfying(&raw_range, &vs))
            };

            let cached = if let Some(ref v) = resolved {
                let db_lock = db.lock().await;
                db_lock
                    .check_cached(&[(name.clone(), v.clone())])
                    .ok()
                    .and_then(|r| r.first().map(|s| s.cached))
                    .unwrap_or(false)
            } else {
                false
            };

            let error = if resolved.is_none() {
                Some("无法解析版本".to_string())
            } else {
                None
            };

            let _ = app.emit(
                "import-resolve-progress",
                ResolveProgressEvent {
                    request_id,
                    name: name.clone(),
                    raw_range: raw_range.clone(),
                    version: resolved.clone(),
                    cached,
                    error,
                },
            );

            resolved.map(|version| ResolvedImportPackage {
                name,
                raw_range,
                version,
                tarball_url: pkg.tarball_url,
                cached,
            })
        }));
    }

    let mut out = Vec::new();
    for h in handles {
        if let Ok(Some(dep)) = h.await {
            out.push(dep);
        }
    }
    Ok(out)
}

#[tauri::command]
async fn resolve_dependencies(packages: Vec<CacheRequest>) -> Result<Vec<ResolvedDep>, String> {
    let initial: Vec<(String, String)> = packages
        .into_iter()
        .map(|p| (p.package_name, p.version))
        .collect();
    dependency_resolver::resolve_all(initial).await
}

#[derive(serde::Serialize, Clone)]
struct ScanProgressEvent {
    count: usize,
    current: String,
}

#[tauri::command]
async fn scan_node_modules(
    app_handle: tauri::AppHandle,
    dir_path: String,
) -> Result<Vec<LocalPackage>, String> {
    use tauri::Emitter;
    let path = PathBuf::from(&dir_path);

    tokio::task::spawn_blocking(move || {
        let mut last_emit = std::time::Instant::now();
        local_scanner::scan_node_modules_with_progress(&path, |count, pkg| {
            // Throttle to ~20 events/sec, but always send the very first one.
            if count == 1 || last_emit.elapsed().as_millis() >= 50 {
                let _ = app_handle.emit(
                    "scan-progress",
                    ScanProgressEvent {
                        count,
                        current: pkg.name.clone(),
                    },
                );
                last_emit = std::time::Instant::now();
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
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

#[derive(Clone, Serialize)]
struct DownloadProgressEvent {
    completed: usize,
    total: usize,
    current: String,
}

#[tauri::command]
async fn download_tarballs(
    app_handle: tauri::AppHandle,
    packages: Vec<CacheRequest>,
    output_dir: String,
) -> Result<usize, String> {
    use tauri::Emitter;
    use tokio::fs;

    let dir = PathBuf::from(&output_dir);
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("创建目录失败: {}", e))?;

    let http = reqwest::Client::new();
    let total = packages.len();
    let mut completed = 0usize;

    for pkg in &packages {
        let name_part = if pkg.package_name.starts_with('@') {
            pkg.package_name.replace('/', "-")
        } else {
            pkg.package_name.clone()
        };
        let filename = format!("{}-{}.tgz", name_part, pkg.version);
        let file_path = dir.join(&filename);

        let _ = app_handle.emit(
            "download-tarball-progress",
            DownloadProgressEvent {
                completed,
                total,
                current: format!("{}@{}", pkg.package_name, pkg.version),
            },
        );

        let tarball_url = format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            pkg.package_name,
            pkg.package_name.split('/').last().unwrap_or(&pkg.package_name),
            pkg.version
        );

        match http.get(&tarball_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
                fs::write(&file_path, &bytes)
                    .await
                    .map_err(|e| format!("写入文件失败: {}", e))?;
                completed += 1;
            }
            Ok(resp) => {
                return Err(format!(
                    "下载 {}@{} 失败 (HTTP {})",
                    pkg.package_name, pkg.version, resp.status()
                ));
            }
            Err(e) => {
                return Err(format!("下载 {}@{} 失败: {}", pkg.package_name, pkg.version, e));
            }
        }
    }

    let _ = app_handle.emit(
        "download-tarball-progress",
        DownloadProgressEvent {
            completed,
            total,
            current: String::new(),
        },
    );

    Ok(completed)
}

#[tauri::command]
async fn check_cached_status(
    state: tauri::State<'_, AppState>,
    packages: Vec<(String, String)>,
) -> Result<Vec<CachedStatus>, String> {
    let db = state.cache_db.lock().await;
    db.check_cached(&packages)
}

#[tauri::command]
async fn get_all_cached_packages(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SearchResult>, String> {
    let db = state.cache_db.lock().await;
    db.get_all_packages()
}

#[tauri::command]
async fn start_cache_sync(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let config = config::load_config(&app_handle);
    let db = state.cache_db.clone();
    let sync = state.sync_engine.clone();

    if sync.is_running() {
        return Err("同步正在进行中".into());
    }

    let app = app_handle.clone();
    tokio::spawn(async move {
        sync.start_sync(
            app,
            config.registry_url,
            config.verdaccio_storage_path,
            db,
        )
        .await;
    });

    Ok(())
}

#[tauri::command]
async fn get_sync_info(
    state: tauri::State<'_, AppState>,
) -> Result<SyncInfo, String> {
    let db = state.cache_db.lock().await;
    let last_registry_url = db.get_meta("last_registry_url")?;
    let last_sync_at = db.get_meta("last_sync_at")?;
    let is_running = state.sync_engine.is_running();
    Ok(SyncInfo {
        last_registry_url,
        last_sync_at,
        is_running,
    })
}

#[tauri::command]
async fn clear_cache_index(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let db = state.cache_db.lock().await;
    db.clear_all()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let db = CacheDb::open(&app.handle())
                .expect("无法打开缓存数据库");
            let cache_db = Arc::new(Mutex::new(db));
            let sync_engine = Arc::new(SyncEngine::new());

            app.manage(AppState {
                task_engine: Arc::new(Mutex::new(None)),
                cache_db: cache_db.clone(),
                sync_engine: sync_engine.clone(),
            });

            // Auto-sync if registry_url changed since last sync
            let config = config::load_config(&app.handle());
            let app_handle = app.handle().clone();
            let db_ref = cache_db.clone();
            let sync_ref = sync_engine.clone();
            tauri::async_runtime::spawn(async move {
                let should_sync = {
                    let db = db_ref.lock().await;
                    let last_url = db.get_meta("last_registry_url").unwrap_or(None);
                    last_url.as_deref() != Some(&config.registry_url)
                };
                if should_sync && !config.registry_url.is_empty() {
                    {
                        let db = db_ref.lock().await;
                        let _ = db.clear_all();
                    }
                    sync_ref
                        .start_sync(
                            app_handle,
                            config.registry_url,
                            config.verdaccio_storage_path,
                            db_ref,
                        )
                        .await;
                }
            });

            Ok(())
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
            resolve_package_versions,
            resolve_dependencies,
            scan_node_modules,
            parse_tgz,
            upload_tgz_files,
            check_cached_status,
            get_all_cached_packages,
            start_cache_sync,
            get_sync_info,
            clear_cache_index,
            download_tarballs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
