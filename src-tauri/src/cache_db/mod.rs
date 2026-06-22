use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::registry_client::SearchResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedStatus {
    pub name: String,
    pub version: String,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncInfo {
    pub last_registry_url: Option<String>,
    pub last_sync_at: Option<String>,
    pub is_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskBatchSummary {
    pub id: String,
    pub source: String,
    pub target_registry: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskItemRecord {
    pub id: String,
    pub batch_id: String,
    pub package_name: String,
    pub version: String,
    pub tarball_url: Option<String>,
    pub status: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attempt_count: u32,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct TaskItemUpdate {
    pub id: String,
    pub status: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attempt_count: u32,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
}

pub struct CacheDb {
    conn: Connection,
}

impl CacheDb {
    pub fn open(app_handle: &AppHandle) -> Result<Self, String> {
        let dir = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| format!("获取 app data 目录失败: {}", e))?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;

        let db_path = dir.join("cache_index.db");
        let conn =
            Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;

        let db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    pub fn open_at(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
        let conn = Connection::open(&path).map_err(|e| format!("打开数据库失败: {}", e))?;
        let db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    fn init_tables(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS packages (
                    name TEXT PRIMARY KEY,
                    description TEXT,
                    latest_version TEXT
                );
                CREATE TABLE IF NOT EXISTS cached_versions (
                    package_name TEXT NOT NULL,
                    version TEXT NOT NULL,
                    PRIMARY KEY (package_name, version)
                );
                CREATE TABLE IF NOT EXISTS sync_metadata (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS task_batches (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    target_registry TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    finished_at TEXT,
                    total INTEGER NOT NULL,
                    success INTEGER NOT NULL DEFAULT 0,
                    failed INTEGER NOT NULL DEFAULT 0,
                    skipped INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS task_items (
                    id TEXT PRIMARY KEY,
                    batch_id TEXT NOT NULL,
                    package_name TEXT NOT NULL,
                    version TEXT NOT NULL,
                    tarball_url TEXT,
                    status TEXT NOT NULL,
                    error_code TEXT,
                    error_message TEXT,
                    attempt_count INTEGER NOT NULL DEFAULT 0,
                    started_at TEXT,
                    finished_at TEXT,
                    duration_ms INTEGER,
                    FOREIGN KEY(batch_id) REFERENCES task_batches(id)
                );",
            )
            .map_err(|e| format!("建表失败: {}", e))
    }

    pub fn replace_all(&self, packages: &[SearchResult]) -> Result<usize, String> {
        self.replace_all_with_progress(packages, |_, _| {})
    }

    pub fn replace_all_with_progress<F>(
        &self,
        packages: &[SearchResult],
        on_progress: F,
    ) -> Result<usize, String>
    where
        F: Fn(usize, usize),
    {
        let total = packages.len();
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("开启事务失败: {}", e))?;

        tx.execute("DELETE FROM cached_versions", [])
            .map_err(|e| format!("清空 cached_versions 失败: {}", e))?;
        tx.execute("DELETE FROM packages", [])
            .map_err(|e| format!("清空 packages 失败: {}", e))?;

        for (i, pkg) in packages.iter().enumerate() {
            tx.execute(
                "INSERT OR REPLACE INTO packages (name, description, latest_version) VALUES (?1, ?2, ?3)",
                params![pkg.name, pkg.description, pkg.latest_version],
            )
            .map_err(|e| format!("插入 package 失败: {}", e))?;

            for ver in &pkg.cached_versions {
                tx.execute(
                    "INSERT OR IGNORE INTO cached_versions (package_name, version) VALUES (?1, ?2)",
                    params![pkg.name, ver],
                )
                .map_err(|e| format!("插入 cached_version 失败: {}", e))?;
            }

            if (i + 1) % 50 == 0 || i + 1 == total {
                on_progress(i + 1, total);
            }
        }

        tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;
        Ok(total)
    }

    pub fn check_cached(&self, queries: &[(String, String)]) -> Result<Vec<CachedStatus>, String> {
        let mut results = Vec::with_capacity(queries.len());
        for (name, version) in queries {
            let exists: bool = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM cached_versions WHERE package_name = ?1 AND version = ?2",
                    params![name, version],
                    |row| row.get::<_, i64>(0).map(|c| c > 0),
                )
                .unwrap_or(false);
            results.push(CachedStatus {
                name: name.clone(),
                version: version.clone(),
                cached: exists,
            });
        }
        Ok(results)
    }

    pub fn get_all_packages(&self) -> Result<Vec<SearchResult>, String> {
        let mut pkg_stmt = self
            .conn
            .prepare("SELECT name, description, latest_version FROM packages")
            .map_err(|e| format!("查询 packages 失败: {}", e))?;

        let mut ver_stmt = self
            .conn
            .prepare("SELECT version FROM cached_versions WHERE package_name = ?1")
            .map_err(|e| format!("准备 cached_versions 查询失败: {}", e))?;

        let packages = pkg_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| format!("查询失败: {}", e))?;

        let mut results = Vec::new();
        for pkg in packages {
            let (name, description, latest_version) =
                pkg.map_err(|e| format!("读取行失败: {}", e))?;

            let cached_versions: Vec<String> = ver_stmt
                .query_map(params![&name], |row| row.get(0))
                .map_err(|e| format!("查询版本失败: {}", e))?
                .filter_map(|r| r.ok())
                .collect();

            results.push(SearchResult {
                name,
                description,
                latest_version,
                versions: cached_versions.clone(),
                cached_versions,
            });
        }
        Ok(results)
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>, String> {
        match self
            .conn
            .query_row(
                "SELECT value FROM sync_metadata WHERE key = ?1",
                params![key],
                |row| row.get(0),
            ) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("查询 metadata 失败: {}", e)),
        }
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO sync_metadata (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| format!("写入 metadata 失败: {}", e))?;
        Ok(())
    }

    pub fn get_existing_packages_map(&self) -> Result<HashMap<String, Vec<String>>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT package_name, version FROM cached_versions ORDER BY package_name")
            .map_err(|e| format!("查询失败: {}", e))?;

        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("查询失败: {}", e))?;

        for row in rows {
            let (name, version) = row.map_err(|e| format!("读取行失败: {}", e))?;
            map.entry(name).or_default().push(version);
        }
        Ok(map)
    }

    pub fn sync_incremental_with_progress<F>(
        &self,
        packages: &[SearchResult],
        on_progress: F,
    ) -> Result<usize, String>
    where
        F: Fn(usize, usize),
    {
        let existing = self.get_existing_packages_map()?;
        let total = packages.len();

        let new_map: HashMap<&str, &SearchResult> =
            packages.iter().map(|p| (p.name.as_str(), p)).collect();

        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("开启事务失败: {}", e))?;

        // Remove packages no longer in source
        for name in existing.keys() {
            if !new_map.contains_key(name.as_str()) {
                tx.execute("DELETE FROM cached_versions WHERE package_name = ?1", params![name])
                    .map_err(|e| format!("删除版本失败: {}", e))?;
                tx.execute("DELETE FROM packages WHERE name = ?1", params![name])
                    .map_err(|e| format!("删除包失败: {}", e))?;
            }
        }

        // Add or update packages
        for (i, pkg) in packages.iter().enumerate() {
            let existing_versions = existing.get(&pkg.name);

            match existing_versions {
                None => {
                    // New package
                    tx.execute(
                        "INSERT INTO packages (name, description, latest_version) VALUES (?1, ?2, ?3)",
                        params![pkg.name, pkg.description, pkg.latest_version],
                    )
                    .map_err(|e| format!("插入包失败: {}", e))?;
                    for ver in &pkg.cached_versions {
                        tx.execute(
                            "INSERT INTO cached_versions (package_name, version) VALUES (?1, ?2)",
                            params![pkg.name, ver],
                        )
                        .map_err(|e| format!("插入版本失败: {}", e))?;
                    }
                }
                Some(old_versions) => {
                    let mut old_sorted = old_versions.clone();
                    old_sorted.sort();
                    let mut new_sorted = pkg.cached_versions.clone();
                    new_sorted.sort();

                    if old_sorted != new_sorted {
                        // Versions changed - update
                        tx.execute(
                            "UPDATE packages SET description = ?2, latest_version = ?3 WHERE name = ?1",
                            params![pkg.name, pkg.description, pkg.latest_version],
                        )
                        .map_err(|e| format!("更新包失败: {}", e))?;
                        tx.execute(
                            "DELETE FROM cached_versions WHERE package_name = ?1",
                            params![pkg.name],
                        )
                        .map_err(|e| format!("删除旧版本失败: {}", e))?;
                        for ver in &pkg.cached_versions {
                            tx.execute(
                                "INSERT INTO cached_versions (package_name, version) VALUES (?1, ?2)",
                                params![pkg.name, ver],
                            )
                            .map_err(|e| format!("插入版本失败: {}", e))?;
                        }
                    }
                }
            }

            if (i + 1) % 50 == 0 || i + 1 == total {
                on_progress(i + 1, total);
            }
        }

        tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;
        Ok(total)
    }

    /// 从本地索引移除指定版本。versions 为 None 时移除整个包。
    /// 当包不再有缓存版本时，一并删除 packages 行。
    pub fn remove_versions(
        &self,
        name: &str,
        versions: Option<&[String]>,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("开启事务失败: {}", e))?;

        match versions {
            None => {
                tx.execute(
                    "DELETE FROM cached_versions WHERE package_name = ?1",
                    params![name],
                )
                .map_err(|e| format!("删除版本失败: {}", e))?;
                tx.execute(
                    "DELETE FROM packages WHERE name = ?1",
                    params![name],
                )
                .map_err(|e| format!("删除包失败: {}", e))?;
            }
            Some(vers) => {
                for ver in vers {
                    tx.execute(
                        "DELETE FROM cached_versions WHERE package_name = ?1 AND version = ?2",
                        params![name, ver],
                    )
                    .map_err(|e| format!("删除版本失败: {}", e))?;
                }
                let remaining: i64 = tx
                    .query_row(
                        "SELECT COUNT(*) FROM cached_versions WHERE package_name = ?1",
                        params![name],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("统计剩余版本失败: {}", e))?;
                if remaining == 0 {
                    tx.execute(
                        "DELETE FROM packages WHERE name = ?1",
                        params![name],
                    )
                    .map_err(|e| format!("删除包失败: {}", e))?;
                }
            }
        }

        tx.commit().map_err(|e| format!("提交事务失败: {}", e))
    }

    pub fn create_task_batch(&self, batch: &TaskBatchSummary) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO task_batches
                    (id, source, target_registry, created_at, finished_at, total, success, failed, skipped)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    batch.id,
                    batch.source,
                    batch.target_registry,
                    batch.created_at,
                    batch.finished_at,
                    batch.total as i64,
                    batch.success as i64,
                    batch.failed as i64,
                    batch.skipped as i64,
                ],
            )
            .map_err(|e| format!("写入任务批次失败: {}", e))?;
        Ok(())
    }

    pub fn insert_task_item(&self, item: &TaskItemRecord) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO task_items
                    (id, batch_id, package_name, version, tarball_url, status, error_code, error_message,
                     attempt_count, started_at, finished_at, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    item.id,
                    item.batch_id,
                    item.package_name,
                    item.version,
                    item.tarball_url,
                    item.status,
                    item.error_code,
                    item.error_message,
                    item.attempt_count as i64,
                    item.started_at,
                    item.finished_at,
                    item.duration_ms.map(|v| v as i64),
                ],
            )
            .map_err(|e| format!("写入任务明细失败: {}", e))?;
        Ok(())
    }

    pub fn update_task_item_status(&self, update: &TaskItemUpdate) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE task_items
                 SET status = ?2,
                     error_code = ?3,
                     error_message = ?4,
                     attempt_count = ?5,
                     started_at = COALESCE(?6, started_at),
                     finished_at = ?7,
                     duration_ms = ?8
                 WHERE id = ?1",
                params![
                    update.id,
                    update.status,
                    update.error_code,
                    update.error_message,
                    update.attempt_count as i64,
                    update.started_at,
                    update.finished_at,
                    update.duration_ms.map(|v| v as i64),
                ],
            )
            .map_err(|e| format!("更新任务状态失败: {}", e))?;
        Ok(())
    }

    pub fn get_task_batches(&self, limit: usize) -> Result<Vec<TaskBatchSummary>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source, target_registry, created_at, finished_at, total, success, failed, skipped
                 FROM task_batches
                 ORDER BY created_at DESC
                 LIMIT ?1",
            )
            .map_err(|e| format!("准备任务批次查询失败: {}", e))?;

        let rows = stmt
            .query_map(params![limit as i64], row_to_task_batch)
            .map_err(|e| format!("查询任务批次失败: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("读取任务批次失败: {}", e))
    }

    pub fn get_task_batch(&self, batch_id: &str) -> Result<Option<TaskBatchSummary>, String> {
        self.conn
            .query_row(
                "SELECT id, source, target_registry, created_at, finished_at, total, success, failed, skipped
                 FROM task_batches
                 WHERE id = ?1",
                params![batch_id],
                row_to_task_batch,
            )
            .optional()
            .map_err(|e| format!("读取任务批次失败: {}", e))
    }

    pub fn get_task_batch_items(&self, batch_id: &str) -> Result<Vec<TaskItemRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, batch_id, package_name, version, tarball_url, status, error_code, error_message,
                        attempt_count, started_at, finished_at, duration_ms
                 FROM task_items
                 WHERE batch_id = ?1
                 ORDER BY package_name, version",
            )
            .map_err(|e| format!("准备任务明细查询失败: {}", e))?;

        let rows = stmt
            .query_map(params![batch_id], row_to_task_item)
            .map_err(|e| format!("查询任务明细失败: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("读取任务明细失败: {}", e))
    }

    pub fn get_failed_task_items(&self, batch_id: &str) -> Result<Vec<TaskItemRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, batch_id, package_name, version, tarball_url, status, error_code, error_message,
                        attempt_count, started_at, finished_at, duration_ms
                 FROM task_items
                 WHERE batch_id = ?1 AND status = 'Failed'
                 ORDER BY package_name, version",
            )
            .map_err(|e| format!("准备失败任务查询失败: {}", e))?;

        let rows = stmt
            .query_map(params![batch_id], row_to_task_item)
            .map_err(|e| format!("查询失败任务失败: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("读取失败任务失败: {}", e))
    }

    pub fn recompute_task_batch_counts(&self, batch_id: &str) -> Result<(), String> {
        let (success, failed, skipped, final_total, latest_finished): (
            i64,
            i64,
            i64,
            i64,
            Option<String>,
        ) = self
            .conn
            .query_row(
                "SELECT
                    SUM(CASE WHEN status = 'Success' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'Failed' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'Skipped' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status IN ('Success', 'Failed', 'Skipped') THEN 1 ELSE 0 END),
                    MAX(finished_at)
                 FROM task_items
                 WHERE batch_id = ?1",
                params![batch_id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                        row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(|e| format!("统计任务批次失败: {}", e))?;

        let total: i64 = self
            .conn
            .query_row(
                "SELECT total FROM task_batches WHERE id = ?1",
                params![batch_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("读取任务批次总数失败: {}", e))?;

        let finished_at = if final_total >= total {
            latest_finished
        } else {
            None
        };

        self.conn
            .execute(
                "UPDATE task_batches
                 SET success = ?2, failed = ?3, skipped = ?4, finished_at = ?5
                 WHERE id = ?1",
                params![batch_id, success, failed, skipped, finished_at],
            )
            .map_err(|e| format!("更新任务批次统计失败: {}", e))?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "DELETE FROM cached_versions;
                 DELETE FROM packages;
                 DELETE FROM sync_metadata;",
            )
            .map_err(|e| format!("清除索引失败: {}", e))
    }
}

fn row_to_task_batch(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskBatchSummary> {
    Ok(TaskBatchSummary {
        id: row.get(0)?,
        source: row.get(1)?,
        target_registry: row.get(2)?,
        created_at: row.get(3)?,
        finished_at: row.get(4)?,
        total: row.get::<_, i64>(5)? as usize,
        success: row.get::<_, i64>(6)? as usize,
        failed: row.get::<_, i64>(7)? as usize,
        skipped: row.get::<_, i64>(8)? as usize,
    })
}

fn row_to_task_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskItemRecord> {
    Ok(TaskItemRecord {
        id: row.get(0)?,
        batch_id: row.get(1)?,
        package_name: row.get(2)?,
        version: row.get(3)?,
        tarball_url: row.get(4)?,
        status: row.get(5)?,
        error_code: row.get(6)?,
        error_message: row.get(7)?,
        attempt_count: row.get::<_, i64>(8)? as u32,
        started_at: row.get(9)?,
        finished_at: row.get(10)?,
        duration_ms: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry_client::SearchResult;

    fn pkg(name: &str, versions: &[&str]) -> SearchResult {
        SearchResult {
            name: name.to_string(),
            description: None,
            latest_version: versions.last().map(|s| s.to_string()),
            versions: versions.iter().map(|s| s.to_string()).collect(),
            cached_versions: versions.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn fresh_db() -> (tempfile::TempDir, CacheDb) {
        let dir = tempfile::tempdir().unwrap();
        let db = CacheDb::open_at(dir.path().join("t.db")).unwrap();
        (dir, db)
    }

    fn cached_versions(db: &CacheDb, name: &str) -> Vec<String> {
        db.get_all_packages()
            .unwrap()
            .into_iter()
            .find(|p| p.name == name)
            .map(|p| p.cached_versions)
            .unwrap_or_default()
    }

    #[test]
    fn remove_single_version_keeps_package_with_remaining() {
        let (_d, db) = fresh_db();
        db.replace_all(&[pkg("left-pad", &["1.0.0", "2.0.0"])])
            .unwrap();

        db.remove_versions("left-pad", Some(&["1.0.0".to_string()]))
            .unwrap();

        assert_eq!(cached_versions(&db, "left-pad"), vec!["2.0.0"]);
    }

    #[test]
    fn remove_last_version_prunes_package_row() {
        let (_d, db) = fresh_db();
        db.replace_all(&[pkg("left-pad", &["1.0.0"])]).unwrap();

        db.remove_versions("left-pad", Some(&["1.0.0".to_string()]))
            .unwrap();

        assert!(db.get_all_packages().unwrap().is_empty());
    }

    #[test]
    fn remove_whole_package() {
        let (_d, db) = fresh_db();
        db.replace_all(&[
            pkg("left-pad", &["1.0.0", "2.0.0"]),
            pkg("right-pad", &["1.0.0"]),
        ])
        .unwrap();

        db.remove_versions("left-pad", None).unwrap();

        let names: Vec<String> = db
            .get_all_packages()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["right-pad"]);
    }

    mod task_history {
        use super::*;

        fn batch(id: &str) -> TaskBatchSummary {
            TaskBatchSummary {
                id: id.to_string(),
                source: "npmjs".to_string(),
                target_registry: "http://localhost:4873".to_string(),
                created_at: "2026-06-22T00:00:00Z".to_string(),
                finished_at: None,
                total: 2,
                success: 0,
                failed: 0,
                skipped: 0,
            }
        }

        fn item(id: &str, batch_id: &str, name: &str, status: &str) -> TaskItemRecord {
            TaskItemRecord {
                id: id.to_string(),
                batch_id: batch_id.to_string(),
                package_name: name.to_string(),
                version: "1.0.0".to_string(),
                tarball_url: None,
                status: status.to_string(),
                error_code: None,
                error_message: None,
                attempt_count: 0,
                started_at: None,
                finished_at: None,
                duration_ms: None,
            }
        }

        #[test]
        fn creates_batch_and_returns_recent_batches() {
            let (_d, db) = fresh_db();

            db.create_task_batch(&batch("batch-a")).unwrap();
            db.create_task_batch(&TaskBatchSummary {
                id: "batch-b".to_string(),
                created_at: "2026-06-22T00:01:00Z".to_string(),
                ..batch("ignored")
            })
            .unwrap();

            let batches = db.get_task_batches(10).unwrap();

            assert_eq!(batches.len(), 2);
            assert_eq!(batches[0].id, "batch-b");
            assert_eq!(batches[1].id, "batch-a");
        }

        #[test]
        fn persists_items_and_updates_final_counts() {
            let (_d, db) = fresh_db();
            db.create_task_batch(&batch("batch-a")).unwrap();
            db.insert_task_item(&item("task-1", "batch-a", "left-pad", "Pending"))
                .unwrap();
            db.insert_task_item(&item("task-2", "batch-a", "right-pad", "Pending"))
                .unwrap();

            db.update_task_item_status(&TaskItemUpdate {
                id: "task-1".to_string(),
                status: "Success".to_string(),
                error_code: None,
                error_message: None,
                attempt_count: 1,
                started_at: Some("2026-06-22T00:00:01Z".to_string()),
                finished_at: Some("2026-06-22T00:00:02Z".to_string()),
                duration_ms: Some(1000),
            })
            .unwrap();
            db.update_task_item_status(&TaskItemUpdate {
                id: "task-2".to_string(),
                status: "Failed".to_string(),
                error_code: Some("PAYLOAD_TOO_LARGE".to_string()),
                error_message: Some("413 Payload Too Large".to_string()),
                attempt_count: 3,
                started_at: Some("2026-06-22T00:00:01Z".to_string()),
                finished_at: Some("2026-06-22T00:00:04Z".to_string()),
                duration_ms: Some(3000),
            })
            .unwrap();
            db.recompute_task_batch_counts("batch-a").unwrap();

            let batch = db.get_task_batch("batch-a").unwrap().unwrap();
            let items = db.get_task_batch_items("batch-a").unwrap();
            let failed = db.get_failed_task_items("batch-a").unwrap();

            assert_eq!(batch.success, 1);
            assert_eq!(batch.failed, 1);
            assert_eq!(batch.skipped, 0);
            assert_eq!(items.len(), 2);
            assert_eq!(failed.len(), 1);
            assert_eq!(failed[0].package_name, "right-pad");
            assert_eq!(failed[0].error_code.as_deref(), Some("PAYLOAD_TOO_LARGE"));
            assert_eq!(failed[0].attempt_count, 3);
        }
    }
}
