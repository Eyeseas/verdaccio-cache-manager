use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha1::Digest as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Semaphore};

use crate::cache_db::{CacheDb, TaskBatchSummary, TaskItemRecord, TaskItemUpdate};
use crate::registry_client::{tarball_url, RegistryClient};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Downloading,
    Uploading,
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskErrorCode {
    #[serde(rename = "AUTH_FAILED")]
    AuthFailed,
    #[serde(rename = "PACKAGE_EXISTS")]
    PackageExists,
    #[serde(rename = "PAYLOAD_TOO_LARGE")]
    PayloadTooLarge,
    #[serde(rename = "NOT_FOUND")]
    NotFound,
    #[serde(rename = "NETWORK_TIMEOUT")]
    NetworkTimeout,
    #[serde(rename = "DOWNLOAD_FAILED")]
    DownloadFailed,
    #[serde(rename = "UPLOAD_FAILED")]
    UploadFailed,
    #[serde(rename = "LOCAL_READ_FAILED")]
    LocalReadFailed,
    #[serde(rename = "PACK_FAILED")]
    PackFailed,
    #[serde(rename = "UNPUBLISH_RETRY_FAILED")]
    UnpublishRetryFailed,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

impl TaskErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskErrorCode::AuthFailed => "AUTH_FAILED",
            TaskErrorCode::PackageExists => "PACKAGE_EXISTS",
            TaskErrorCode::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            TaskErrorCode::NotFound => "NOT_FOUND",
            TaskErrorCode::NetworkTimeout => "NETWORK_TIMEOUT",
            TaskErrorCode::DownloadFailed => "DOWNLOAD_FAILED",
            TaskErrorCode::UploadFailed => "UPLOAD_FAILED",
            TaskErrorCode::LocalReadFailed => "LOCAL_READ_FAILED",
            TaskErrorCode::PackFailed => "PACK_FAILED",
            TaskErrorCode::UnpublishRetryFailed => "UNPUBLISH_RETRY_FAILED",
            TaskErrorCode::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStage {
    Download,
    Upload,
    LocalRead,
    Pack,
    ProxyCache,
    UnpublishRetry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheTask {
    pub id: String,
    pub batch_id: String,
    pub package_name: String,
    pub version: String,
    pub tarball_url: Option<String>,
    pub status: TaskStatus,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub attempt_count: u32,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct TaskProgressEvent {
    pub id: String,
    pub batch_id: String,
    pub package_name: String,
    pub version: String,
    pub status: TaskStatus,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub attempt_count: u32,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
}

pub fn classify_task_error(stage: TaskStage, message: &str) -> TaskErrorCode {
    let lower = message.to_ascii_lowercase();
    if message.contains("401")
        || message.contains("403")
        || message.contains("权限不足")
        || message.contains("登录失败")
    {
        return TaskErrorCode::AuthFailed;
    }
    if message.contains("409")
        || lower.contains("conflict")
        || lower.contains("already exist")
        || message.contains("版本已存在")
    {
        return TaskErrorCode::PackageExists;
    }
    if message.contains("413") || lower.contains("payload too large") {
        return TaskErrorCode::PayloadTooLarge;
    }
    if message.contains("404") || message.contains("不存在") || lower.contains("not found") {
        return TaskErrorCode::NotFound;
    }
    if message.contains("超时") || lower.contains("timeout") {
        return TaskErrorCode::NetworkTimeout;
    }

    match stage {
        TaskStage::Download | TaskStage::ProxyCache => TaskErrorCode::DownloadFailed,
        TaskStage::Upload => TaskErrorCode::UploadFailed,
        TaskStage::LocalRead => TaskErrorCode::LocalReadFailed,
        TaskStage::Pack => TaskErrorCode::PackFailed,
        TaskStage::UnpublishRetry => TaskErrorCode::UnpublishRetryFailed,
    }
}

pub fn format_task_batch_markdown_report(
    batch: &TaskBatchSummary,
    items: &[TaskItemRecord],
) -> String {
    let mut out = String::new();
    out.push_str("# Task Batch Report\n\n");
    out.push_str(&format!("- Batch ID: {}\n", batch.id));
    out.push_str(&format!("- Source: {}\n", batch.source));
    out.push_str(&format!("- Target Registry: {}\n", batch.target_registry));
    out.push_str(&format!("- Created At: {}\n", batch.created_at));
    out.push_str(&format!(
        "- Finished At: {}\n",
        batch.finished_at.as_deref().unwrap_or("")
    ));
    out.push_str(&format!("- Total: {}\n", batch.total));
    out.push_str(&format!("- Success: {}\n", batch.success));
    out.push_str(&format!("- Skipped: {}\n", batch.skipped));
    out.push_str(&format!("- Failed: {}\n\n", batch.failed));

    out.push_str("## Failures\n\n");
    out.push_str("| Package | Error Code | Attempts | Duration | Error |\n");
    out.push_str("|---|---|---:|---:|---|\n");
    for item in items.iter().filter(|item| item.status == "Failed") {
        out.push_str(&format!(
            "| {}@{} | {} | {} | {} | {} |\n",
            escape_markdown_table_cell(&item.package_name),
            escape_markdown_table_cell(&item.version),
            item.error_code.as_deref().unwrap_or(""),
            item.attempt_count,
            format_duration(item.duration_ms),
            escape_markdown_table_cell(item.error_message.as_deref().unwrap_or(""))
        ));
    }

    out.push_str("\n## All Tasks\n\n");
    out.push_str("| Package | Status | Attempts | Duration | Error Code |\n");
    out.push_str("|---|---|---:|---:|---|\n");
    for item in items {
        out.push_str(&format!(
            "| {}@{} | {} | {} | {} | {} |\n",
            escape_markdown_table_cell(&item.package_name),
            escape_markdown_table_cell(&item.version),
            item.status,
            item.attempt_count,
            format_duration(item.duration_ms),
            item.error_code.as_deref().unwrap_or("")
        ));
    }
    out
}

fn format_duration(duration_ms: Option<u64>) -> String {
    duration_ms
        .map(|ms| format!("{}ms", ms))
        .unwrap_or_default()
}

fn escape_markdown_table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

pub struct TaskEngine {
    pub tasks: Arc<Mutex<Vec<CacheTask>>>,
    semaphore: Arc<Semaphore>,
    retry_count: u32,
    timeout_secs: u64,
    history_db: Option<Arc<Mutex<CacheDb>>>,
}

impl TaskEngine {
    pub fn new(concurrency: u32, retry_count: u32, timeout_secs: u64) -> Self {
        Self::new_with_history(concurrency, retry_count, timeout_secs, None)
    }

    pub fn new_with_history(
        concurrency: u32,
        retry_count: u32,
        timeout_secs: u64,
        history_db: Option<Arc<Mutex<CacheDb>>>,
    ) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
            semaphore: Arc::new(Semaphore::new(concurrency as usize)),
            retry_count,
            timeout_secs,
            history_db,
        }
    }

    pub async fn add_task(&self, task: CacheTask) {
        let mut tasks = self.tasks.lock().await;
        tasks.push(task);
    }

    pub async fn get_tasks(&self) -> Vec<CacheTask> {
        let tasks = self.tasks.lock().await;
        tasks.clone()
    }

    pub async fn clear_completed(&self) {
        let mut tasks = self.tasks.lock().await;
        tasks.retain(|t| t.status != TaskStatus::Success && t.status != TaskStatus::Skipped);
    }

    pub async fn retry_failed(&self) {
        let mut tasks = self.tasks.lock().await;
        for task in tasks.iter_mut() {
            if task.status == TaskStatus::Failed {
                task.status = TaskStatus::Pending;
                task.error = None;
                task.error_code = None;
                task.attempt_count = 0;
                task.started_at = None;
                task.finished_at = None;
                task.duration_ms = None;
            }
        }
    }

    pub async fn execute_all(
        &self,
        app_handle: AppHandle,
        source_registry: String,
        target_registry: String,
    ) {
        let tasks_snapshot = {
            let tasks = self.tasks.lock().await;
            tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Pending)
                .cloned()
                .collect::<Vec<_>>()
        };

        let source_client = Arc::new(RegistryClient::new(&source_registry));
        let target_client = Arc::new(RegistryClient::new(&target_registry));
        // 预先登录目标 registry，避免并发任务重复触发
        // PUT /-/user/org.couchdb.user:cache-manager 而打满 409 日志
        let _ = target_client.ensure_token().await;

        let mut handles = Vec::new();

        for task in tasks_snapshot {
            let sem = self.semaphore.clone();
            let tasks_ref = self.tasks.clone();
            let app = app_handle.clone();
            let source = source_client.clone();
            let target = target_client.clone();
            let retry_count = self.retry_count;
            let timeout_secs = self.timeout_secs;
            let history_db = self.history_db.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                execute_single_task(
                    task,
                    tasks_ref,
                    app,
                    source,
                    target,
                    retry_count,
                    timeout_secs,
                    history_db,
                )
                .await;
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }
    }
}

async fn execute_single_task(
    task: CacheTask,
    tasks: Arc<Mutex<Vec<CacheTask>>>,
    app: AppHandle,
    source_client: Arc<RegistryClient>,
    target_client: Arc<RegistryClient>,
    retry_count: u32,
    timeout_secs: u64,
    history_db: Option<Arc<Mutex<CacheDb>>>,
) {
    let run_meta = TaskRunMeta::new();
    let is_local_source = task.tarball_url.as_ref().map_or(false, |u| {
        u.starts_with("file://") || u.starts_with("dir://")
    });
    let use_proxy_cache =
        !is_local_source && source_client.registry_url.contains("npmjs.org");

    if use_proxy_cache {
        execute_proxy_cache(
            task,
            tasks,
            app,
            target_client,
            retry_count,
            timeout_secs,
            history_db,
            run_meta,
        )
        .await;
    } else {
        execute_publish(
            task,
            tasks,
            app,
            source_client,
            target_client,
            retry_count,
            timeout_secs,
            history_db,
            run_meta,
        )
        .await;
    }
}

async fn execute_proxy_cache(
    task: CacheTask,
    tasks: Arc<Mutex<Vec<CacheTask>>>,
    app: AppHandle,
    target_client: Arc<RegistryClient>,
    retry_count: u32,
    timeout_secs: u64,
    history_db: Option<Arc<Mutex<CacheDb>>>,
    run_meta: TaskRunMeta,
) {
    update_task_status(
        &tasks,
        &task.id,
        TaskStatus::Downloading,
        None,
        None,
        0,
        &run_meta,
        &app,
        history_db.as_ref(),
    )
    .await;

    for attempt in 0..=retry_count {
        match tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            target_client.trigger_proxy_cache(&task.package_name, &task.version),
        )
        .await
        {
            Ok(Ok(())) => {
                update_task_status(
                    &tasks,
                    &task.id,
                    TaskStatus::Success,
                    None,
                    None,
                    attempt + 1,
                    &run_meta,
                    &app,
                    history_db.as_ref(),
                )
                .await;
                return;
            }
            Ok(Err(e)) => {
                if attempt == retry_count {
                    update_task_status(
                        &tasks,
                        &task.id,
                        TaskStatus::Failed,
                        Some(format!("缓存失败: {}", e)),
                        Some(TaskStage::ProxyCache),
                        attempt + 1,
                        &run_meta,
                        &app,
                        history_db.as_ref(),
                    )
                    .await;
                    return;
                }
            }
            Err(_) => {
                if attempt == retry_count {
                    update_task_status(
                        &tasks,
                        &task.id,
                        TaskStatus::Failed,
                        Some("缓存超时".to_string()),
                        Some(TaskStage::ProxyCache),
                        attempt + 1,
                        &run_meta,
                        &app,
                        history_db.as_ref(),
                    )
                    .await;
                    return;
                }
            }
        }
    }
}

async fn execute_publish(
    task: CacheTask,
    tasks: Arc<Mutex<Vec<CacheTask>>>,
    app: AppHandle,
    source_client: Arc<RegistryClient>,
    target_client: Arc<RegistryClient>,
    retry_count: u32,
    timeout_secs: u64,
    history_db: Option<Arc<Mutex<CacheDb>>>,
    run_meta: TaskRunMeta,
) {
    let tarball_url = match &task.tarball_url {
        Some(url) => url.clone(),
        None => tarball_url(
            &source_client.registry_url,
            &task.package_name,
            &task.version,
        ),
    };

    update_task_status(
        &tasks,
        &task.id,
        TaskStatus::Downloading,
        None,
        None,
        0,
        &run_meta,
        &app,
        history_db.as_ref(),
    )
    .await;

    let tarball_data = if tarball_url.starts_with("dir://") {
        let dir_path = &tarball_url[6..];
        match pack_directory(dir_path) {
            Ok(data) => data,
            Err(e) => {
                update_task_status(
                    &tasks,
                    &task.id,
                    TaskStatus::Failed,
                    Some(format!("打包失败: {}", e)),
                    Some(TaskStage::Pack),
                    1,
                    &run_meta,
                    &app,
                    history_db.as_ref(),
                )
                .await;
                return;
            }
        }
    } else if tarball_url.starts_with("file://") {
        let file_path = &tarball_url[7..];
        match std::fs::read(file_path) {
            Ok(data) => data,
            Err(e) => {
                update_task_status(
                    &tasks,
                    &task.id,
                    TaskStatus::Failed,
                    Some(format!("读取文件失败: {}", e)),
                    Some(TaskStage::LocalRead),
                    1,
                    &run_meta,
                    &app,
                    history_db.as_ref(),
                )
                .await;
                return;
            }
        }
    } else {
        let mut data = None;
        for attempt in 0..=retry_count {
            match tokio::time::timeout(
                Duration::from_secs(timeout_secs),
                source_client.download_tarball(&tarball_url),
            )
            .await
            {
                Ok(Ok(d)) => {
                    data = Some(d);
                    break;
                }
                Ok(Err(e)) => {
                    if attempt == retry_count {
                        update_task_status(
                            &tasks,
                            &task.id,
                            TaskStatus::Failed,
                            Some(format!("下载失败: {}", e)),
                            Some(TaskStage::Download),
                            attempt + 1,
                            &run_meta,
                            &app,
                            history_db.as_ref(),
                        )
                        .await;
                        return;
                    }
                }
                Err(_) => {
                    if attempt == retry_count {
                        update_task_status(
                            &tasks,
                            &task.id,
                            TaskStatus::Failed,
                            Some("下载超时".to_string()),
                            Some(TaskStage::Download),
                            attempt + 1,
                            &run_meta,
                            &app,
                            history_db.as_ref(),
                        )
                        .await;
                        return;
                    }
                }
            }
        }
        data.unwrap()
    };

    update_task_status(
        &tasks,
        &task.id,
        TaskStatus::Uploading,
        None,
        None,
        0,
        &run_meta,
        &app,
        history_db.as_ref(),
    )
    .await;

    let shasum = sha1::Sha1::digest(&tarball_data)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tarball_data))
    );

    // Extract full package.json from tarball to preserve dependencies metadata
    let mut metadata = match extract_package_json_from_tarball(&tarball_data) {
        Ok(mut pkg) => {
            if let Some(obj) = pkg.as_object_mut() {
                obj.insert("name".to_string(), serde_json::json!(task.package_name));
                obj.insert("version".to_string(), serde_json::json!(task.version));
            }
            pkg
        }
        Err(_) => {
            serde_json::json!({
                "name": task.package_name,
                "version": task.version,
            })
        }
    };

    metadata["dist"] = serde_json::json!({
        "tarball": crate::registry_client::tarball_url(&target_client.registry_url, &task.package_name, &task.version),
        "shasum": shasum,
        "integrity": integrity
    });

    for attempt in 0..=retry_count {
        match tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            target_client.publish_package(
                &task.package_name,
                &task.version,
                metadata.clone(),
                &tarball_data,
            ),
        )
        .await
        {
            Ok(Ok(())) => {
                update_task_status(
                    &tasks,
                    &task.id,
                    TaskStatus::Success,
                    None,
                    None,
                    attempt + 1,
                    &run_meta,
                    &app,
                    history_db.as_ref(),
                )
                .await;
                return;
            }
            Ok(Err(e)) => {
                if e.contains("409") || e.contains("conflict") || e.contains("already exist") {
                    let is_local = tarball_url.starts_with("file://") || tarball_url.starts_with("dir://");
                    if !is_local {
                        // Remote source: trust that the version genuinely exists
                        update_task_status(
                            &tasks,
                            &task.id,
                            TaskStatus::Skipped,
                            Some("版本已存在".to_string()),
                            Some(TaskStage::Upload),
                            attempt + 1,
                            &run_meta,
                            &app,
                            history_db.as_ref(),
                        )
                        .await;
                        return;
                    }

                    // Local source (dir:// or file://): Verdaccio may only have metadata
                    // from uplink without a local tarball. Unpublish then re-publish to
                    // ensure the tarball is physically stored.
                    let _ = target_client
                        .unpublish_package(&task.package_name, Some(&task.version))
                        .await;

                    match tokio::time::timeout(
                        Duration::from_secs(timeout_secs),
                        target_client.publish_package(
                            &task.package_name,
                            &task.version,
                            metadata.clone(),
                            &tarball_data,
                        ),
                    )
                    .await
                    {
                        Ok(Ok(())) => {
                            update_task_status(
                                &tasks,
                                &task.id,
                                TaskStatus::Success,
                                None,
                                None,
                                attempt + 1,
                                &run_meta,
                                &app,
                                history_db.as_ref(),
                            )
                            .await;
                            return;
                        }
                        Ok(Err(e2)) => {
                            update_task_status(
                                &tasks,
                                &task.id,
                                TaskStatus::Failed,
                                Some(format!("unpublish 后重试仍失败: {}", e2)),
                                Some(TaskStage::UnpublishRetry),
                                attempt + 1,
                                &run_meta,
                                &app,
                                history_db.as_ref(),
                            )
                            .await;
                            return;
                        }
                        Err(_) => {
                            update_task_status(
                                &tasks,
                                &task.id,
                                TaskStatus::Failed,
                                Some("unpublish 后重试上传超时".to_string()),
                                Some(TaskStage::UnpublishRetry),
                                attempt + 1,
                                &run_meta,
                                &app,
                                history_db.as_ref(),
                            )
                            .await;
                            return;
                        }
                    }
                }
                if attempt == retry_count {
                    update_task_status(
                        &tasks,
                        &task.id,
                        TaskStatus::Failed,
                        Some(format!("上传失败: {}", e)),
                        Some(TaskStage::Upload),
                        attempt + 1,
                        &run_meta,
                        &app,
                        history_db.as_ref(),
                    )
                    .await;
                    return;
                }
            }
            Err(_) => {
                if attempt == retry_count {
                    update_task_status(
                        &tasks,
                        &task.id,
                        TaskStatus::Failed,
                        Some("上传超时".to_string()),
                        Some(TaskStage::Upload),
                        attempt + 1,
                        &run_meta,
                        &app,
                        history_db.as_ref(),
                    )
                    .await;
                    return;
                }
            }
        }
    }
}

fn extract_package_json_from_tarball(tarball_data: &[u8]) -> Result<serde_json::Value, String> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    use tar::Archive;

    let gz = GzDecoder::new(tarball_data);
    let mut archive = Archive::new(gz);

    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();

        // npm tarballs store package.json at "package/package.json"
        if path.ends_with("package.json")
            && path.components().count() == 2
        {
            let mut content = String::new();
            entry.read_to_string(&mut content).map_err(|e| e.to_string())?;
            let json: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| e.to_string())?;
            return Ok(json);
        }
    }

    Err("tarball 中未找到 package.json".to_string())
}

async fn update_task_status(
    tasks: &Arc<Mutex<Vec<CacheTask>>>,
    task_id: &str,
    status: TaskStatus,
    error: Option<String>,
    error_stage: Option<TaskStage>,
    attempt_count: u32,
    run_meta: &TaskRunMeta,
    app: &AppHandle,
    history_db: Option<&Arc<Mutex<CacheDb>>>,
) {
    let now = timestamp_millis();
    let is_final = matches!(
        status,
        TaskStatus::Success | TaskStatus::Failed | TaskStatus::Skipped
    );
    let error_code = error
        .as_deref()
        .and_then(|msg| error_stage.map(|stage| classify_task_error(stage, msg).as_str().to_string()));
    let duration_ms = is_final.then_some(run_meta.started_instant.elapsed().as_millis() as u64);
    let finished_at = is_final.then(|| now.clone());

    let updated = {
        let mut tasks = tasks.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = status;
            task.error = error.clone();
            task.error_code = error_code.clone();
            task.attempt_count = attempt_count;
            if task.started_at.is_none() {
                task.started_at = Some(run_meta.started_at.clone());
            }
            task.finished_at = finished_at.clone();
            task.duration_ms = duration_ms;
            Some(task.clone())
        } else {
            None
        }
    };

    if let Some(task) = updated {
        if let Some(db) = history_db {
            let db = db.lock().await;
            let _ = db.update_task_item_status(&TaskItemUpdate {
                id: task.id.clone(),
                status: format!("{:?}", task.status),
                error_code: task.error_code.clone(),
                error_message: task.error.clone(),
                attempt_count: task.attempt_count,
                started_at: task.started_at.clone(),
                finished_at: task.finished_at.clone(),
                duration_ms: task.duration_ms,
            });
            if is_final {
                let _ = db.recompute_task_batch_counts(&task.batch_id);
            }
        }

        let _ = app.emit(
            "task-progress",
            TaskProgressEvent {
                id: task.id,
                batch_id: task.batch_id,
                package_name: task.package_name,
                version: task.version,
                status,
                error,
                error_code,
                attempt_count,
                started_at: task.started_at,
                finished_at,
                duration_ms,
            },
        );
    }
}

#[derive(Clone)]
struct TaskRunMeta {
    started_at: String,
    started_instant: std::time::Instant,
}

impl TaskRunMeta {
    fn new() -> Self {
        Self {
            started_at: timestamp_millis(),
            started_instant: std::time::Instant::now(),
        }
    }
}

pub fn timestamp_millis() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn pack_directory(dir_path: &str) -> Result<Vec<u8>, String> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use walkdir::WalkDir;

    let dir = Path::new(dir_path);
    if !dir.exists() {
        return Err(format!("目录不存在: {}", dir_path));
    }

    let mut gz_buf = Vec::new();
    {
        let gz = GzEncoder::new(&mut gz_buf, Compression::default());
        let mut tar_builder = tar::Builder::new(gz);

        for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let relative = path.strip_prefix(dir).unwrap_or(path);

            if relative.as_os_str().is_empty() {
                continue;
            }

            // npm tarball convention: files are under "package/" prefix
            let archive_path = Path::new("package").join(relative);

            if path.is_file() {
                tar_builder
                    .append_path_with_name(path, &archive_path)
                    .map_err(|e| format!("添加文件到 tar 失败: {}", e))?;
            } else if path.is_dir() && relative.as_os_str() != "" {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                header.set_mtime(0);
                header.set_cksum();
                tar_builder
                    .append_data(&mut header, &archive_path, &[][..])
                    .map_err(|e| format!("添加目录到 tar 失败: {}", e))?;
            }
        }

        let gz = tar_builder
            .into_inner()
            .map_err(|e| format!("完成 tar 失败: {}", e))?;
        gz.finish().map_err(|e| format!("完成 gzip 失败: {}", e))?;
    }

    Ok(gz_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache_db::{TaskBatchSummary, TaskItemRecord};

    #[test]
    fn classifies_common_task_errors() {
        assert_eq!(
            classify_task_error(TaskStage::Upload, "Publish 失败 (413 Payload Too Large)"),
            TaskErrorCode::PayloadTooLarge
        );
        assert_eq!(
            classify_task_error(TaskStage::Upload, "Publish 失败 (409 Conflict)"),
            TaskErrorCode::PackageExists
        );
        assert_eq!(
            classify_task_error(TaskStage::Download, "下载超时"),
            TaskErrorCode::NetworkTimeout
        );
        assert_eq!(
            classify_task_error(TaskStage::LocalRead, "读取文件失败: access denied"),
            TaskErrorCode::LocalReadFailed
        );
    }

    #[test]
    fn cache_task_carries_durable_batch_metadata() {
        let task = CacheTask {
            id: "task-1".to_string(),
            batch_id: "batch-1".to_string(),
            package_name: "left-pad".to_string(),
            version: "1.0.0".to_string(),
            tarball_url: None,
            status: TaskStatus::Pending,
            error: None,
            error_code: None,
            attempt_count: 0,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        };

        assert_eq!(task.batch_id, "batch-1");
        assert_eq!(task.attempt_count, 0);
        assert!(task.error_code.is_none());
    }

    #[test]
    fn formats_markdown_report_with_failures_and_all_tasks() {
        let batch = TaskBatchSummary {
            id: "batch-1".to_string(),
            source: "npmjs".to_string(),
            target_registry: "http://localhost:4873".to_string(),
            created_at: "2026-06-22T00:00:00Z".to_string(),
            finished_at: Some("2026-06-22T00:01:00Z".to_string()),
            total: 2,
            success: 1,
            failed: 1,
            skipped: 0,
        };
        let items = vec![
            TaskItemRecord {
                id: "task-1".to_string(),
                batch_id: "batch-1".to_string(),
                package_name: "left-pad".to_string(),
                version: "1.0.0".to_string(),
                tarball_url: None,
                status: "Success".to_string(),
                error_code: None,
                error_message: None,
                attempt_count: 1,
                started_at: None,
                finished_at: Some("2026-06-22T00:00:01Z".to_string()),
                duration_ms: Some(500),
            },
            TaskItemRecord {
                id: "task-2".to_string(),
                batch_id: "batch-1".to_string(),
                package_name: "right-pad".to_string(),
                version: "1.0.0".to_string(),
                tarball_url: None,
                status: "Failed".to_string(),
                error_code: Some("PAYLOAD_TOO_LARGE".to_string()),
                error_message: Some("413 Payload Too Large".to_string()),
                attempt_count: 3,
                started_at: None,
                finished_at: Some("2026-06-22T00:00:04Z".to_string()),
                duration_ms: Some(3000),
            },
        ];

        let report = format_task_batch_markdown_report(&batch, &items);

        assert!(report.contains("# Task Batch Report"));
        assert!(report.contains("- Batch ID: batch-1"));
        assert!(report.contains("| right-pad@1.0.0 | PAYLOAD_TOO_LARGE | 3 | 3000ms | 413 Payload Too Large |"));
        assert!(report.contains("| left-pad@1.0.0 | Success | 1 | 500ms |  |"));
    }
}
