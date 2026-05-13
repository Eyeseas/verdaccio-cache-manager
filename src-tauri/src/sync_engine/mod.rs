use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::cache_db::CacheDb;
use crate::registry_client::RegistryClient;
use crate::storage_scanner;

#[derive(Clone, serde::Serialize)]
pub struct SyncStatusEvent {
    pub status: String,
    pub progress: usize,
    pub total: usize,
    pub message: Option<String>,
}

pub struct SyncEngine {
    running: AtomicBool,
    cancel: AtomicBool,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub async fn start_sync(
        &self,
        app_handle: AppHandle,
        registry_url: String,
        storage_path: Option<String>,
        db: Arc<Mutex<CacheDb>>,
    ) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        self.cancel.store(false, Ordering::Relaxed);

        let _ = app_handle.emit(
            "sync-status",
            SyncStatusEvent {
                status: "started".into(),
                progress: 0,
                total: 0,
                message: None,
            },
        );

        let result = self
            .do_sync(&app_handle, &registry_url, storage_path.as_deref(), &db)
            .await;

        match result {
            Ok(count) => {
                let db_lock = db.lock().await;
                let _ = db_lock.set_meta("last_registry_url", &registry_url);
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .to_string();
                let _ = db_lock.set_meta("last_sync_at", &ts);
                drop(db_lock);

                let _ = app_handle.emit(
                    "sync-status",
                    SyncStatusEvent {
                        status: "done".into(),
                        progress: count,
                        total: count,
                        message: None,
                    },
                );
            }
            Err(e) => {
                let _ = app_handle.emit(
                    "sync-status",
                    SyncStatusEvent {
                        status: "error".into(),
                        progress: 0,
                        total: 0,
                        message: Some(e),
                    },
                );
            }
        }

        self.running.store(false, Ordering::SeqCst);
    }

    async fn do_sync(
        &self,
        app_handle: &AppHandle,
        registry_url: &str,
        storage_path: Option<&str>,
        db: &Arc<Mutex<CacheDb>>,
    ) -> Result<usize, String> {
        let client = RegistryClient::new(registry_url);

        let _ = app_handle.emit(
            "sync-status",
            SyncStatusEvent {
                status: "fetching".into(),
                progress: 0,
                total: 0,
                message: Some("正在获取包列表...".into()),
            },
        );

        let packages = match client.list_cached_via_plugin().await {
            Ok(pkgs) => pkgs,
            Err(e) => {
                if let Some(path) = storage_path {
                    let p = std::path::PathBuf::from(path);
                    storage_scanner::scan_storage(&p)?
                } else {
                    return Err(format!("插件不可用且未配置 storage 路径: {}", e));
                }
            }
        };

        if self.cancel.load(Ordering::Relaxed) {
            return Err("同步已取消".into());
        }

        let total = packages.len();
        let db_lock = db.lock().await;
        let is_incremental = db_lock.get_meta("last_sync_at")?.is_some();

        let msg = if is_incremental {
            "正在更新本地索引..."
        } else {
            "正在写入本地索引..."
        };

        let _ = app_handle.emit(
            "sync-status",
            SyncStatusEvent {
                status: "progress".into(),
                progress: 0,
                total,
                message: Some(msg.into()),
            },
        );

        let app_clone = app_handle.clone();
        let progress_cb = |current: usize, t: usize| {
            let _ = app_clone.emit(
                "sync-status",
                SyncStatusEvent {
                    status: "progress".into(),
                    progress: current,
                    total: t,
                    message: None,
                },
            );
        };

        if is_incremental {
            db_lock.sync_incremental_with_progress(&packages, progress_cb)?;
        } else {
            db_lock.replace_all_with_progress(&packages, progress_cb)?;
        }
        drop(db_lock);

        Ok(total)
    }
}
