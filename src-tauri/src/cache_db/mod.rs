use rusqlite::{params, Connection};
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