use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Semaphore};

use crate::registry_client::RegistryClient;

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

        let mut handles = Vec::new();

        for task in tasks_snapshot {
            let sem = self.semaphore.clone();
            let tasks_ref = self.tasks.clone();
            let app = app_handle.clone();
            let source = source_registry.clone();
            let target = target_registry.clone();
            let retry_count = self.retry_count;
            let timeout_secs = self.timeout_secs;

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                execute_single_task(
                    task,
                    tasks_ref,
                    app,
                    &source,
                    &target,
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
    source_registry: &str,
    target_registry: &str,
    retry_count: u32,
    timeout_secs: u64,
) {
    let source_client = RegistryClient::new(source_registry);
    let mut target_client = RegistryClient::new(target_registry);

    let tarball_url = match &task.tarball_url {
        Some(url) => url.clone(),
        None => format!(
            "{}/{}/-/{}-{}.tgz",
            source_registry, task.package_name, task.package_name, task.version
        ),
    };

    update_task_status(&tasks, &task.id, TaskStatus::Downloading, None, &app).await;

    let mut tarball_data = None;
    for attempt in 0..=retry_count {
        match tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            source_client.download_tarball(&tarball_url),
        )
        .await
        {
            Ok(Ok(data)) => {
                tarball_data = Some(data);
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

    let tarball_data = tarball_data.unwrap();

    update_task_status(&tasks, &task.id, TaskStatus::Uploading, None, &app).await;

    let metadata = serde_json::json!({
        "name": task.package_name,
        "version": task.version,
        "dist": {
            "tarball": format!("{}/{}/-/{}-{}.tgz", target_registry, task.package_name, task.package_name, task.version)
        }
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
                    update_task_status(&tasks, &task.id, TaskStatus::Skipped, Some("版本已存在".to_string()), &app).await;
                    return;
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

async fn update_task_status(
    tasks: &Arc<Mutex<Vec<CacheTask>>>,
    task_id: &str,
    status: TaskStatus,
    error: Option<String>,
    app: &AppHandle,
) {
    {
        let mut tasks = tasks.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = status;
            task.error = error.clone();
        }
    }

    let _ = app.emit(
        "task-progress",
        TaskProgressEvent {
            id: task_id.to_string(),
            package_name: String::new(),
            version: String::new(),
            status,
            error,
        },
    );
}
