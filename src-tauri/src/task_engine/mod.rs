use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha1::Digest as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Semaphore};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheTask {
    pub id: String,
    pub package_name: String,
    pub version: String,
    pub tarball_url: Option<String>,
    pub status: TaskStatus,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct TaskProgressEvent {
    pub id: String,
    pub package_name: String,
    pub version: String,
    pub status: TaskStatus,
    pub error: Option<String>,
}

pub struct TaskEngine {
    pub tasks: Arc<Mutex<Vec<CacheTask>>>,
    semaphore: Arc<Semaphore>,
    retry_count: u32,
    timeout_secs: u64,
}

impl TaskEngine {
    pub fn new(concurrency: u32, retry_count: u32, timeout_secs: u64) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
            semaphore: Arc::new(Semaphore::new(concurrency as usize)),
            retry_count,
            timeout_secs,
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
) {
    let is_local_source = task.tarball_url.as_ref().map_or(false, |u| {
        u.starts_with("file://") || u.starts_with("dir://")
    });
    let use_proxy_cache =
        !is_local_source && source_client.registry_url.contains("npmjs.org");

    if use_proxy_cache {
        execute_proxy_cache(task, tasks, app, target_client, retry_count, timeout_secs).await;
    } else {
        execute_publish(
            task,
            tasks,
            app,
            source_client,
            target_client,
            retry_count,
            timeout_secs,
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
) {
    update_task_status(&tasks, &task.id, TaskStatus::Downloading, None, &app).await;

    for attempt in 0..=retry_count {
        match tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            target_client.trigger_proxy_cache(&task.package_name, &task.version),
        )
        .await
        {
            Ok(Ok(())) => {
                update_task_status(&tasks, &task.id, TaskStatus::Success, None, &app).await;
                return;
            }
            Ok(Err(e)) => {
                if attempt == retry_count {
                    update_task_status(
                        &tasks,
                        &task.id,
                        TaskStatus::Failed,
                        Some(format!("缓存失败: {}", e)),
                        &app,
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
                        &app,
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
) {
    let tarball_url = match &task.tarball_url {
        Some(url) => url.clone(),
        None => tarball_url(
            &source_client.registry_url,
            &task.package_name,
            &task.version,
        ),
    };

    update_task_status(&tasks, &task.id, TaskStatus::Downloading, None, &app).await;

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
                    &app,
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
                    &app,
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
                            &app,
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
                            &app,
                        )
                        .await;
                        return;
                    }
                }
            }
        }
        data.unwrap()
    };

    update_task_status(&tasks, &task.id, TaskStatus::Uploading, None, &app).await;

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
                update_task_status(&tasks, &task.id, TaskStatus::Success, None, &app).await;
                return;
            }
            Ok(Err(e)) => {
                if e.contains("409") || e.contains("conflict") || e.contains("already exist") {
                    let is_local = tarball_url.starts_with("file://") || tarball_url.starts_with("dir://");
                    if !is_local {
                        // Remote source: trust that the version genuinely exists
                        update_task_status(&tasks, &task.id, TaskStatus::Skipped, Some("版本已存在".to_string()), &app).await;
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
                            update_task_status(&tasks, &task.id, TaskStatus::Success, None, &app).await;
                            return;
                        }
                        Ok(Err(e2)) => {
                            update_task_status(
                                &tasks,
                                &task.id,
                                TaskStatus::Failed,
                                Some(format!("unpublish 后重试仍失败: {}", e2)),
                                &app,
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
                                &app,
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
                        &app,
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
                        &app,
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
    app: &AppHandle,
) {
    let (package_name, version) = {
        let mut tasks = tasks.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = status;
            task.error = error.clone();
            (task.package_name.clone(), task.version.clone())
        } else {
            (String::new(), String::new())
        }
    };

    let _ = app.emit(
        "task-progress",
        TaskProgressEvent {
            id: task_id.to_string(),
            package_name,
            version,
            status,
            error,
        },
    );
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
