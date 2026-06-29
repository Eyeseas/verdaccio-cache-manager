pub mod cache_db;
pub mod config;
pub mod dependency_resolver;
pub mod local_scanner;
pub mod package_downgrade;
pub mod parser;
pub mod registry_client;
pub mod storage_scanner;
pub mod sync_engine;
pub mod task_engine;

use cache_db::{CacheDb, CachedStatus, SyncInfo, TaskBatchSummary, TaskItemRecord};
use config::AppConfig;
use dependency_resolver::ResolvedDep;
use local_scanner::LocalPackage;
use package_downgrade::{DowngradeAnalysis, OverwriteResult, SavePathResult};
use parser::ParsedDependency;
use registry_client::SearchResult;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use sync_engine::SyncEngine;
use tauri::Manager;
use task_engine::{CacheTask, TaskEngine, TaskStatus};
use tokio::sync::Mutex;

struct AppState {
    task_engine: Arc<Mutex<Option<Arc<TaskEngine>>>>,
    cache_db: Arc<Mutex<CacheDb>>,
    sync_engine: Arc<SyncEngine>,
}

const VERDACCIO_PLUGIN_RESOURCE_PATH: &str = "resources/verdaccio-cached-list.tgz";

#[derive(Debug, Clone, Serialize)]
struct VerdaccioPluginInfo {
    name: String,
    version: String,
    filename: String,
}

fn verdaccio_plugin_info_from_package(pkg: LocalPackage) -> VerdaccioPluginInfo {
    let filename = format!("{}-{}.tgz", pkg.name, pkg.version);
    VerdaccioPluginInfo {
        name: pkg.name,
        version: pkg.version,
        filename,
    }
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

fn normalize_node_version_stdout(stdout: &[u8]) -> Result<String, String> {
    let version = String::from_utf8_lossy(stdout).trim().to_string();
    if version.is_empty() {
        Err("node -v 未返回版本".to_string())
    } else {
        Ok(version)
    }
}

#[tauri::command]
fn get_node_version() -> Result<String, String> {
    let output = Command::new("node")
        .arg("-v")
        .output()
        .map_err(|e| format!("无法执行 node -v: {}", e))?;
    if !output.status.success() {
        return Err("node -v 执行失败".to_string());
    }
    normalize_node_version_stdout(&output.stdout)
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

fn cache_requests_from_failed_items(items: Vec<TaskItemRecord>) -> Vec<CacheRequest> {
    items
        .into_iter()
        .map(|item| CacheRequest {
            package_name: item.package_name,
            version: item.version,
            tarball_url: item.tarball_url,
        })
        .collect()
}

fn format_task_batch_json_report(
    batch: &TaskBatchSummary,
    items: &[TaskItemRecord],
) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "batch": batch,
        "items": items,
    }))
    .map_err(|e| format!("生成 JSON 报告失败: {}", e))
}

#[tauri::command]
async fn start_cache_tasks(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    packages: Vec<CacheRequest>,
) -> Result<String, String> {
    let config = config::load_config(&app_handle);

    let batch_id = format!("batch-{}", chrono_id());
    let engine = TaskEngine::new_with_history(
        config.concurrency,
        config.retry_count,
        config.timeout_secs,
        Some(state.cache_db.clone()),
    );

    for (i, pkg) in packages.iter().enumerate() {
        let task = CacheTask {
            id: format!("task-{}-{}", chrono_id(), i),
            batch_id: batch_id.clone(),
            package_name: pkg.package_name.clone(),
            version: pkg.version.clone(),
            tarball_url: pkg.tarball_url.clone(),
            status: TaskStatus::Pending,
            error: None,
            error_code: None,
            attempt_count: 0,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        };
        engine.add_task(task).await;
    }

    let source_registry = "https://registry.npmjs.org".to_string();
    let target_registry = config.registry_url.clone();
    persist_initial_task_batch(
        &state.cache_db,
        &batch_id,
        "npmjs",
        &target_registry,
        &engine.get_tasks().await,
    )
    .await?;

    let engine = Arc::new(engine);
    {
        let mut lock = state.task_engine.lock().await;
        *lock = Some(engine.clone());
    }

    let app = app_handle.clone();

    tokio::spawn(async move {
        engine.execute_all(app, source_registry, target_registry).await;
    });

    Ok(batch_id)
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
async fn get_task_batches(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<TaskBatchSummary>, String> {
    let db = state.cache_db.lock().await;
    db.get_task_batches(limit.unwrap_or(20))
}

#[tauri::command]
async fn get_task_batch_items(
    state: tauri::State<'_, AppState>,
    batch_id: String,
) -> Result<Vec<TaskItemRecord>, String> {
    let db = state.cache_db.lock().await;
    db.get_task_batch_items(&batch_id)
}

#[tauri::command]
async fn get_current_task_batch(
    state: tauri::State<'_, AppState>,
) -> Result<Option<TaskBatchSummary>, String> {
    let batch_id = {
        let lock = state.task_engine.lock().await;
        match lock.as_ref() {
            Some(engine) => engine
                .get_tasks()
                .await
                .first()
                .map(|task| task.batch_id.clone()),
            None => None,
        }
    };

    match batch_id {
        Some(id) => {
            let db = state.cache_db.lock().await;
            db.get_task_batch(&id)
        }
        None => Ok(None),
    }
}

#[tauri::command]
async fn retry_batch_failed_tasks(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    batch_id: String,
) -> Result<String, String> {
    let failed = {
        let db = state.cache_db.lock().await;
        db.get_failed_task_items(&batch_id)?
    };
    if failed.is_empty() {
        return Err("该批次没有失败任务".to_string());
    }
    let requests = cache_requests_from_failed_items(failed);
    start_cache_tasks(app_handle, state, requests).await
}

#[tauri::command]
async fn export_task_batch_report(
    state: tauri::State<'_, AppState>,
    batch_id: String,
    output_path: String,
    format: String,
) -> Result<String, String> {
    use tokio::fs;

    let (batch, items) = {
        let db = state.cache_db.lock().await;
        let batch = db
            .get_task_batch(&batch_id)?
            .ok_or_else(|| format!("任务批次不存在: {}", batch_id))?;
        let items = db.get_task_batch_items(&batch_id)?;
        (batch, items)
    };

    let content = match format.as_str() {
        "markdown" | "md" => task_engine::format_task_batch_markdown_report(&batch, &items),
        "json" => format_task_batch_json_report(&batch, &items)?,
        other => return Err(format!("不支持的报告格式: {}", other)),
    };

    let output = PathBuf::from(output_path);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建报告目录失败: {}", e))?;
    }
    fs::write(&output, content)
        .await
        .map_err(|e| format!("写入报告失败: {}", e))?;
    Ok(output.to_string_lossy().to_string())
}

#[tauri::command]
async fn retry_failed_tasks(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let config = config::load_config(&app_handle);

    let engine = {
        let lock = state.task_engine.lock().await;
        lock.clone()
    };

    if let Some(ref eng) = engine {
        eng.retry_failed().await;

        let source_registry = "https://registry.npmjs.org".to_string();
        let target_registry = config.registry_url.clone();
        let app = app_handle.clone();
        let eng = eng.clone();

        tokio::spawn(async move {
            eng.execute_all(app, source_registry, target_registry).await;
        });
    }

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

async fn persist_initial_task_batch(
    db: &Arc<Mutex<CacheDb>>,
    batch_id: &str,
    source: &str,
    target_registry: &str,
    tasks: &[CacheTask],
) -> Result<(), String> {
    let created_at = task_engine::timestamp_millis();
    let db = db.lock().await;
    db.create_task_batch(&TaskBatchSummary {
        id: batch_id.to_string(),
        source: source.to_string(),
        target_registry: target_registry.to_string(),
        created_at,
        finished_at: None,
        total: tasks.len(),
        success: 0,
        failed: 0,
        skipped: 0,
    })?;

    for task in tasks {
        db.insert_task_item(&TaskItemRecord {
            id: task.id.clone(),
            batch_id: task.batch_id.clone(),
            package_name: task.package_name.clone(),
            version: task.version.clone(),
            tarball_url: task.tarball_url.clone(),
            status: format!("{:?}", task.status),
            error_code: task.error_code.clone(),
            error_message: task.error.clone(),
            attempt_count: task.attempt_count,
            started_at: task.started_at.clone(),
            finished_at: task.finished_at.clone(),
            duration_ms: task.duration_ms,
        })?;
    }

    Ok(())
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

            let resolved =
                parser::resolve_single(&http, &sem, &cache, &name, &raw_range).await;

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
    dependency_resolver::resolve_all(initial, "https://registry.npmjs.org").await
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
) -> Result<String, String> {
    let config = config::load_config(&app_handle);
    let batch_id = format!("batch-{}", chrono_id());
    let engine = TaskEngine::new_with_history(
        config.concurrency,
        config.retry_count,
        config.timeout_secs,
        Some(state.cache_db.clone()),
    );

    for (i, fp) in file_paths.iter().enumerate() {
        let path = PathBuf::from(fp);
        let pkg = local_scanner::parse_tgz_metadata(&path)?;
        let task = CacheTask {
            id: format!("upload-{}-{}", chrono_id(), i),
            batch_id: batch_id.clone(),
            package_name: pkg.name,
            version: pkg.version,
            tarball_url: Some(format!("file://{}", fp)),
            status: TaskStatus::Pending,
            error: None,
            error_code: None,
            attempt_count: 0,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        };
        engine.add_task(task).await;
    }

    let target_registry = config.registry_url.clone();
    persist_initial_task_batch(
        &state.cache_db,
        &batch_id,
        "local",
        &target_registry,
        &engine.get_tasks().await,
    )
    .await?;

    let engine = Arc::new(engine);
    {
        let mut lock = state.task_engine.lock().await;
        *lock = Some(engine.clone());
    }

    let app = app_handle.clone();

    tokio::spawn(async move {
        engine.execute_all(app, "file://local".to_string(), target_registry)
            .await;
    });

    Ok(batch_id)
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
) -> Result<registry_client::DownloadSummary, String> {
    use tauri::Emitter;

    let pkgs: Vec<(String, String)> = packages
        .into_iter()
        .map(|p| (p.package_name, p.version))
        .collect();
    let app = app_handle.clone();

    registry_client::download_tarballs_to_dir(
        &pkgs,
        &PathBuf::from(&output_dir),
        move |completed, total, current| {
            let _ = app.emit(
                "download-tarball-progress",
                DownloadProgressEvent {
                    completed,
                    total,
                    current: current.to_string(),
                },
            );
        },
    )
    .await
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
async fn analyze_package_json_downgrade(
    state: tauri::State<'_, AppState>,
    file_path: String,
    allow_major_downgrade: bool,
    request_id: Option<String>,
) -> Result<DowngradeAnalysis, String> {
    let db = state.cache_db.lock().await;
    let packages = db.get_all_packages()?;
    let cached = package_downgrade::cached_map_from_packages(&packages);
    package_downgrade::analyze_file(
        &file_path,
        &cached,
        allow_major_downgrade,
        request_id,
    )
}

#[tauri::command]
fn save_downgraded_package_json(
    output_path: String,
    content: String,
) -> Result<SavePathResult, String> {
    let path = package_downgrade::save_content_to_path(&PathBuf::from(&output_path), &content)?;
    Ok(SavePathResult {
        output_path: path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
fn overwrite_package_json(file_path: String, content: String) -> Result<OverwriteResult, String> {
    package_downgrade::overwrite_with_backup(&PathBuf::from(file_path), &content)
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

#[tauri::command]
async fn unpublish_package(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
    version: Option<String>,
) -> Result<(), String> {
    let config = config::load_config(&app_handle);
    let client = registry_client::RegistryClient::new(&config.registry_url);
    client
        .unpublish_package(&name, version.as_deref())
        .await?;

    let db = state.cache_db.lock().await;
    match &version {
        Some(v) => db.remove_versions(&name, Some(std::slice::from_ref(v)))?,
        None => db.remove_versions(&name, None)?,
    }
    Ok(())
}

#[tauri::command]
async fn deprecate_package(
    app_handle: tauri::AppHandle,
    name: String,
    version: String,
    message: String,
) -> Result<(), String> {
    let config = config::load_config(&app_handle);
    let client = registry_client::RegistryClient::new(&config.registry_url);
    client
        .deprecate_package(&name, &version, &message)
        .await
}

#[tauri::command]
async fn export_verdaccio_plugin(
    app_handle: tauri::AppHandle,
    output_path: String,
) -> Result<String, String> {
    use tokio::fs;

    let resource_path = app_handle
        .path()
        .resolve(VERDACCIO_PLUGIN_RESOURCE_PATH, tauri::path::BaseDirectory::Resource)
        .map_err(|e| format!("无法定位内置插件包: {}", e))?;

    let output = PathBuf::from(&output_path);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建导出目录失败: {}", e))?;
    }

    fs::copy(&resource_path, &output)
        .await
        .map_err(|e| format!("导出插件包失败: {}", e))?;

    Ok(output.to_string_lossy().to_string())
}

#[tauri::command]
async fn get_verdaccio_plugin_info(
    app_handle: tauri::AppHandle,
) -> Result<VerdaccioPluginInfo, String> {
    let resource_path = app_handle
        .path()
        .resolve(VERDACCIO_PLUGIN_RESOURCE_PATH, tauri::path::BaseDirectory::Resource)
        .map_err(|e| format!("无法定位内置插件包: {}", e))?;

    let pkg = local_scanner::parse_tgz_metadata(&resource_path)
        .map_err(|e| format!("读取内置插件包信息失败: {}", e))?;

    Ok(verdaccio_plugin_info_from_package(pkg))
}

#[cfg(test)]
mod plugin_export_tests {
    use super::*;
    use cache_db::TaskItemRecord;

    #[test]
    fn verdaccio_plugin_resource_path_is_stable_across_plugin_versions() {
        assert_eq!(VERDACCIO_PLUGIN_RESOURCE_PATH, "resources/verdaccio-cached-list.tgz");
    }

    #[test]
    fn verdaccio_plugin_info_uses_package_metadata_for_export_filename() {
        let pkg = LocalPackage {
            name: "verdaccio-cached-list".to_string(),
            version: "0.2.0".to_string(),
            path: PathBuf::from("ignored.tgz"),
        };

        let info = verdaccio_plugin_info_from_package(pkg);

        assert_eq!(info.name, "verdaccio-cached-list");
        assert_eq!(info.version, "0.2.0");
        assert_eq!(info.filename, "verdaccio-cached-list-0.2.0.tgz");
    }

    #[test]
    fn retry_requests_use_failed_task_identity_and_tarball_url() {
        let failed = vec![TaskItemRecord {
            id: "old-task".to_string(),
            batch_id: "old-batch".to_string(),
            package_name: "left-pad".to_string(),
            version: "1.0.0".to_string(),
            tarball_url: Some("file://left-pad.tgz".to_string()),
            status: "Failed".to_string(),
            error_code: Some("UPLOAD_FAILED".to_string()),
            error_message: Some("boom".to_string()),
            attempt_count: 3,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        }];

        let requests = cache_requests_from_failed_items(failed);

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].package_name, "left-pad");
        assert_eq!(requests[0].version, "1.0.0");
        assert_eq!(requests[0].tarball_url.as_deref(), Some("file://left-pad.tgz"));
    }

    #[test]
    fn task_batch_json_report_contains_summary_and_items() {
        let batch = TaskBatchSummary {
            id: "batch-1".to_string(),
            source: "npmjs".to_string(),
            target_registry: "http://localhost:4873".to_string(),
            created_at: "1".to_string(),
            finished_at: Some("2".to_string()),
            total: 1,
            success: 0,
            failed: 1,
            skipped: 0,
        };
        let items = vec![TaskItemRecord {
            id: "task-1".to_string(),
            batch_id: "batch-1".to_string(),
            package_name: "left-pad".to_string(),
            version: "1.0.0".to_string(),
            tarball_url: None,
            status: "Failed".to_string(),
            error_code: Some("UPLOAD_FAILED".to_string()),
            error_message: Some("boom".to_string()),
            attempt_count: 1,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        }];

        let json = format_task_batch_json_report(&batch, &items).unwrap();

        assert!(json.contains("\"batch\""));
        assert!(json.contains("\"items\""));
        assert!(json.contains("\"left-pad\""));
    }

    #[test]
    fn normalize_node_version_stdout_trims_version() {
        let version = normalize_node_version_stdout(b"v22.12.0\n").unwrap();
        assert_eq!(version, "v22.12.0");
    }

    #[test]
    fn normalize_node_version_stdout_rejects_empty_output() {
        let err = normalize_node_version_stdout(b" \n").unwrap_err();
        assert!(err.contains("node -v 未返回版本"));
    }
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
            get_node_version,
            search_packages,
            list_cached_via_plugin,
            scan_verdaccio_storage,
            get_package_versions,
            get_cached_versions,
            start_cache_tasks,
            get_tasks,
            get_task_batches,
            get_task_batch_items,
            get_current_task_batch,
            retry_batch_failed_tasks,
            export_task_batch_report,
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
            analyze_package_json_downgrade,
            save_downgraded_package_json,
            overwrite_package_json,
            start_cache_sync,
            get_sync_info,
            clear_cache_index,
            download_tarballs,
            unpublish_package,
            deprecate_package,
            export_verdaccio_plugin,
            get_verdaccio_plugin_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
