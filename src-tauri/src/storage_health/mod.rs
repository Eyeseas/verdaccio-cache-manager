//! Verdaccio storage 体检：找出三类会导致离线安装缺包的元数据问题。
//!
//! 1. 幽灵版本：元数据里列出、但本地没有 tarball。通常来自曾经联网时缓存的
//!    uplink 元数据，npm 选中后 Verdaccio 转去 uplink 拉取，离线时返回 500。
//! 2. latest 过时：npm 只要 `dist-tags.latest` 满足范围就优先选它，即使有更高
//!    版本。latest 停在旧的 uplink 缓存版本上时，后补的新版本永远不会被选中。
//! 3. 孤儿 tarball：有 tgz 但元数据未登记该版本，见 [`crate::orphan_repair`]。

use crate::registry_client::RegistryClient;
use crate::storage_scanner::{ensure_storage_dir, list_cached_versions, package_dirs};
use node_semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageHealth {
    pub name: String,
    /// 元数据中存在、但本地无 tarball 的版本（升序）
    pub missing_versions: Vec<String>,
    /// 元数据中存在且本地有 tarball 的版本（升序）
    pub cached_versions: Vec<String>,
    /// 本地有 tarball、但元数据未登记的版本（升序）
    #[serde(default)]
    pub orphan_versions: Vec<String>,
    pub latest: Option<String>,
    /// 修复后 latest 应指向的版本：本地可用（含待登记孤儿）的最高正式版
    pub expected_latest: Option<String>,
    /// dist-tags.latest 指向的版本没有 tarball
    pub latest_missing: bool,
    /// dist-tags.latest 与 expected_latest 不一致
    pub latest_stale: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    /// 扫描到的包目录数
    pub scanned: usize,
    /// 存在问题的包，按名称排序
    pub unhealthy: Vec<PackageHealth>,
    /// package.json 无法读取或解析的包目录
    pub unreadable: Vec<String>,
    /// 远程模式下插件版本过旧（< 0.2.0）：看不到无 tarball 的包，latest 判断不精确
    pub plugin_outdated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction {
    /// 已剔除幽灵版本和/或修正 latest
    Repaired,
    /// 包没有任何 tarball，已移除其元数据（备份后删除 package.json）
    Removed,
    /// 无需处理
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairOutcome {
    pub name: String,
    pub action: RepairAction,
    pub removed_versions: Vec<String>,
    /// 已登记进元数据的孤儿版本
    pub registered_versions: Vec<String>,
    pub previous_latest: Option<String>,
    /// latest 被修改时的新值
    pub new_latest: Option<String>,
    pub backup_path: Option<String>,
    pub message: Option<String>,
}

impl RepairOutcome {
    fn new(name: &str, action: RepairAction) -> Self {
        Self {
            name: name.to_string(),
            action,
            removed_versions: vec![],
            registered_versions: vec![],
            previous_latest: None,
            new_latest: None,
            backup_path: None,
            message: None,
        }
    }
}

pub fn check_storage(storage_path: &Path) -> Result<HealthReport, String> {
    ensure_storage_dir(storage_path)?;

    let dirs = package_dirs(storage_path)?;
    let mut unhealthy = Vec::new();
    let mut unreadable = Vec::new();
    let mut scanned = 0usize;

    for dir in &dirs {
        if !dir.join("package.json").exists() {
            continue;
        }
        scanned += 1;
        match inspect_package(dir) {
            Ok(Some(health)) => unhealthy.push(health),
            Ok(None) => {}
            Err(_) => unreadable.push(dir.to_string_lossy().to_string()),
        }
    }

    unhealthy.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(HealthReport {
        scanned,
        unhealthy,
        unreadable,
        plugin_outdated: false,
    })
}

/// 远程体检：通过 Verdaccio 内的 verdaccio-cached-list 插件读取 storage 状态，
/// 适用于 storage 在远端服务器 / Docker 卷中、本机无法直接访问的情况。
pub async fn check_remote(client: &RegistryClient) -> Result<HealthReport, String> {
    let entries = client.fetch_plugin_entries(true).await.map_err(|e| {
        if e == "PLUGIN_NOT_INSTALLED" {
            "Verdaccio 未安装 verdaccio-cached-list 插件，请在设置页导出插件包并安装".to_string()
        } else {
            e
        }
    })?;

    let plugin_outdated = entries
        .first()
        .is_some_and(|e| e.get("dist_latest").is_none());
    let str_arr = |entry: &serde_json::Value, key: &str| -> Vec<String> {
        entry[key]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };

    let mut unhealthy: Vec<PackageHealth> = entries
        .iter()
        .filter_map(|entry| {
            let name = entry["name"].as_str()?.to_string();
            // 旧插件没有 dist_latest，只能退回到 latest（缺失时它会被填成最高版本）
            let latest = entry
                .get("dist_latest")
                .unwrap_or(&entry["latest"])
                .as_str()
                .map(str::to_string);
            assess(
                name,
                str_arr(entry, "versions"),
                &str_arr(entry, "cached_versions"),
                latest,
            )
        })
        .collect();

    unhealthy.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(HealthReport {
        scanned: entries.len(),
        unhealthy,
        unreadable: vec![],
        plugin_outdated,
    })
}

/// 远程修复：按体检结果通过 Registry API 修改元数据。
/// 无 tarball 的包整包删除；其余包一次 PUT 剔除幽灵版本并修正 latest。
pub async fn repair_remote(client: &RegistryClient, packages: Vec<PackageHealth>) -> Vec<RepairOutcome> {
    let mut outcomes = Vec::with_capacity(packages.len());
    for pkg in packages {
        // 先登记孤儿：登记成功的才算可用版本，参与 latest 取值
        let mut registered = Vec::new();
        let mut orphan_errors = Vec::new();
        for v in &pkg.orphan_versions {
            match crate::orphan_repair::repair_from_registry(client, &pkg.name, v).await {
                Ok(()) => registered.push(v.clone()),
                Err(e) => orphan_errors.push(format!("{}: {}", v, e)),
            }
        }
        if !orphan_errors.is_empty() {
            outcomes.push(RepairOutcome {
                registered_versions: registered,
                message: Some(format!("孤儿版本登记失败（{}），未继续修复", orphan_errors.join("；"))),
                ..RepairOutcome::new(&pkg.name, RepairAction::Failed)
            });
            continue;
        }

        let mut available = pkg.cached_versions.clone();
        available.extend(registered.iter().cloned());
        sort_versions(&mut available);
        let new_latest = preferred_latest(&available).filter(|v| pkg.latest.as_ref() != Some(v));
        let result = if available.is_empty() {
            client
                .unpublish_package(&pkg.name, None)
                .await
                .map(|_| RepairAction::Removed)
        } else if pkg.missing_versions.is_empty() && new_latest.is_none() {
            Ok(if registered.is_empty() { RepairAction::Skipped } else { RepairAction::Repaired })
        } else {
            client
                .prune_versions(&pkg.name, &pkg.missing_versions, new_latest.as_deref())
                .await
                .map(|_| RepairAction::Repaired)
        };

        outcomes.push(match result {
            Ok(RepairAction::Skipped) => RepairOutcome::new(&pkg.name, RepairAction::Skipped),
            Ok(action) => RepairOutcome {
                name: pkg.name,
                action,
                removed_versions: pkg.missing_versions,
                registered_versions: registered,
                previous_latest: pkg.latest,
                new_latest,
                backup_path: None,
                message: None,
            },
            Err(e) => RepairOutcome {
                message: Some(e),
                ..RepairOutcome::new(&pkg.name, RepairAction::Failed)
            },
        });
    }
    outcomes
}

/// `registry_url` 仅用于登记孤儿版本时写入 dist.tarball（Verdaccio 输出时只取其文件名）。
pub fn repair_packages(
    storage_path: &Path,
    names: &[String],
    registry_url: &str,
) -> Result<Vec<RepairOutcome>, String> {
    ensure_storage_dir(storage_path)?;
    Ok(names
        .iter()
        .map(|name| {
            repair_package(storage_path, name, registry_url).unwrap_or_else(|e| RepairOutcome {
                message: Some(e),
                ..RepairOutcome::new(name, RepairAction::Failed)
            })
        })
        .collect())
}

fn read_manifest(pkg_dir: &Path) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(pkg_dir.join("package.json"))
        .map_err(|e| format!("读取 package.json 失败: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("解析 package.json 失败: {}", e))
}

fn manifest_versions(json: &serde_json::Value) -> Vec<String> {
    let mut versions: Vec<String> = json["versions"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    sort_versions(&mut versions);
    versions
}

/// 按本地 tarball 把元数据版本分成（可用, 缺失）两组，均为升序。
fn partition_versions(
    json: &serde_json::Value,
    pkg_dir: &Path,
    name: &str,
) -> (Vec<String>, Vec<String>) {
    let tarballs: HashSet<String> = list_cached_versions(pkg_dir, name).into_iter().collect();
    manifest_versions(json)
        .into_iter()
        .partition(|v| tarballs.contains(v))
}

fn latest_of(json: &serde_json::Value) -> Option<String> {
    json["dist-tags"]["latest"].as_str().map(str::to_string)
}

fn inspect_package(pkg_dir: &Path) -> Result<Option<PackageHealth>, String> {
    let json = read_manifest(pkg_dir)?;
    let name = json["name"]
        .as_str()
        .ok_or_else(|| "package.json 缺少 name".to_string())?
        .to_string();

    let tarballs = list_cached_versions(pkg_dir, &name);
    Ok(assess(name, manifest_versions(&json), &tarballs, latest_of(&json)))
}

/// 判定单个包是否有问题。`versions` 为元数据中的版本，`tarballs` 为本地实际
/// 存在 tarball 的版本，`latest` 为原始 dist-tags.latest。本地与远程模式共用。
fn assess(
    name: String,
    versions: Vec<String>,
    tarballs: &[String],
    latest: Option<String>,
) -> Option<PackageHealth> {
    let tarball_set: HashSet<&str> = tarballs.iter().map(String::as_str).collect();
    let (_, mut orphan_versions) =
        crate::orphan_repair::split_orphans(&versions, tarballs.to_vec());
    let (mut cached_versions, mut missing_versions): (Vec<String>, Vec<String>) = versions
        .into_iter()
        .partition(|v| tarball_set.contains(v.as_str()));
    sort_versions(&mut cached_versions);
    sort_versions(&mut missing_versions);
    sort_versions(&mut orphan_versions);
    let mut available = [cached_versions.clone(), orphan_versions.clone()].concat();
    sort_versions(&mut available);
    let expected_latest = preferred_latest(&available);
    let latest_missing = latest
        .as_deref()
        .is_some_and(|v| !cached_versions.iter().any(|c| c == v));
    let latest_stale = expected_latest.is_some() && latest != expected_latest;

    if missing_versions.is_empty() && orphan_versions.is_empty() && !latest_stale {
        return None;
    }

    Some(PackageHealth {
        name,
        missing_versions,
        cached_versions,
        orphan_versions,
        latest,
        expected_latest,
        latest_missing,
        latest_stale,
    })
}

/// 包名映射到 storage 下的目录，拒绝可能越出 storage 的名称。
fn package_dir(storage_path: &Path, name: &str) -> Result<PathBuf, String> {
    let rel = Path::new(name);
    let valid = !name.is_empty()
        && rel
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && rel.components().count() <= 2;
    if !valid {
        return Err(format!("非法包名: {}", name));
    }
    Ok(storage_path.join(rel))
}

/// 把孤儿 tarball 直接写入元数据（版本条目与 `_attachments`），返回登记的版本。
/// 任一文件内容与文件名不符即整体放弃，不修改元数据。
fn register_orphans(
    json: &mut serde_json::Value,
    pkg_dir: &Path,
    name: &str,
    registry_url: &str,
) -> Result<Vec<String>, String> {
    let (_, orphans) = crate::orphan_repair::split_orphans(
        &manifest_versions(json),
        list_cached_versions(pkg_dir, name),
    );
    for v in &orphans {
        let filename = crate::registry_client::tarball_filename(name, v);
        let data = std::fs::read(pkg_dir.join(&filename))
            .map_err(|e| format!("读取 {} 失败: {}", filename, e))?;
        crate::orphan_repair::verify_identity(name, v, &data)
            .map_err(|e| format!("{}: {}", filename, e))?;
        let url = crate::registry_client::tarball_url(registry_url, name, v);
        if !json["versions"].is_object() {
            json["versions"] = serde_json::json!({});
        }
        json["versions"][v] = crate::orphan_repair::version_metadata(name, v, &data, url);
        if !json["_attachments"].is_object() {
            json["_attachments"] = serde_json::json!({});
        }
        json["_attachments"][&filename] = serde_json::json!({
            "shasum": crate::orphan_repair::shasum(&data),
            "version": v,
        });
    }
    Ok(orphans)
}

fn repair_package(storage_path: &Path, name: &str, registry_url: &str) -> Result<RepairOutcome, String> {
    let pkg_dir = package_dir(storage_path, name)?;
    let manifest_path = pkg_dir.join("package.json");
    let mut json = read_manifest(&pkg_dir)?;
    let registered = register_orphans(&mut json, &pkg_dir, name, registry_url)?;

    let (cached, missing) = partition_versions(&json, &pkg_dir, name);
    let previous_latest = latest_of(&json);

    if cached.is_empty() && missing.is_empty() {
        return Ok(RepairOutcome {
            message: Some("元数据中没有任何版本".to_string()),
            ..RepairOutcome::new(name, RepairAction::Skipped)
        });
    }

    prune_manifest(&mut json, name, &missing);
    let expected = preferred_latest(&cached);
    let new_latest = expected.filter(|v| previous_latest.as_ref() != Some(v));
    if let Some(v) = &new_latest {
        json["dist-tags"]["latest"] = serde_json::Value::String(v.clone());
    }

    if missing.is_empty() && new_latest.is_none() && registered.is_empty() {
        return Ok(RepairOutcome {
            message: Some("元数据与 tarball 一致，latest 已是最新".to_string()),
            ..RepairOutcome::new(name, RepairAction::Skipped)
        });
    }

    let backup_path = backup_path_for(&manifest_path);
    std::fs::copy(&manifest_path, &backup_path).map_err(|e| format!("创建备份失败: {}", e))?;

    let action = if cached.is_empty() {
        // 一个 tarball 都没有：留下空壳元数据只会继续触发 uplink 回源，
        // 直接移除，让 Verdaccio 返回 404，后续可正常 publish 补包。
        std::fs::remove_file(&manifest_path)
            .map_err(|e| format!("移除 package.json 失败: {}", e))?;
        RepairAction::Removed
    } else {
        let content = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
        std::fs::write(&manifest_path, content)
            .map_err(|e| format!("写入 package.json 失败: {}", e))?;
        RepairAction::Repaired
    };

    Ok(RepairOutcome {
        name: name.to_string(),
        action,
        removed_versions: missing,
        registered_versions: registered,
        previous_latest,
        new_latest,
        backup_path: Some(backup_path.to_string_lossy().to_string()),
        message: None,
    })
}

/// 从 Verdaccio 包元数据中剔除 `removed` 版本，同步清理 `time`、`_distfiles`，
/// 并删除指向已剔除版本的非 latest dist-tag（latest 由调用方统一设置）。
fn prune_manifest(json: &mut serde_json::Value, name: &str, removed: &[String]) {
    if removed.is_empty() {
        return;
    }
    let removed_set: HashSet<&str> = removed.iter().map(String::as_str).collect();

    if let Some(versions) = json["versions"].as_object_mut() {
        versions.retain(|v, _| !removed_set.contains(v.as_str()));
    }
    if let Some(time) = json.get_mut("time").and_then(|t| t.as_object_mut()) {
        time.retain(|k, _| !removed_set.contains(k.as_str()));
    }
    let name_part = name.rsplit('/').next().unwrap_or(name);
    if let Some(distfiles) = json.get_mut("_distfiles").and_then(|d| d.as_object_mut()) {
        for v in removed {
            distfiles.remove(&format!("{}-{}.tgz", name_part, v));
        }
    }
    if let Some(tags) = json.get_mut("dist-tags").and_then(|t| t.as_object_mut()) {
        tags.retain(|tag, v| {
            tag == "latest" || v.as_str().is_some_and(|v| !removed_set.contains(v))
        });
    }
}

/// 优先选最高的正式版，没有正式版时退回最高版本。
fn preferred_latest(sorted_versions: &[String]) -> Option<String> {
    sorted_versions
        .iter()
        .rev()
        .find(|v| Version::parse(v).is_ok_and(|p| !p.is_prerelease()))
        .or_else(|| sorted_versions.last())
        .cloned()
}

fn sort_versions(versions: &mut [String]) {
    versions.sort_by(|a, b| match (Version::parse(a), Version::parse(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb),
        _ => a.cmp(b),
    });
}

fn backup_path_for(path: &Path) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    path.with_file_name(format!("package.json.{}.bak", ts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_pkg(storage: &Path, name: &str, manifest: serde_json::Value, tarballs: &[&str]) -> PathBuf {
        let dir = storage.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
        let name_part = name.rsplit('/').next().unwrap();
        for v in tarballs {
            std::fs::write(dir.join(format!("{}-{}.tgz", name_part, v)), b"tgz").unwrap();
        }
        dir
    }

    fn balanced_match_manifest() -> serde_json::Value {
        json!({
            "name": "balanced-match",
            "versions": { "1.0.2": {}, "4.0.2": {}, "4.0.3": {}, "4.0.4": {} },
            "dist-tags": { "latest": "4.0.3", "next": "4.0.3" },
            "time": { "modified": "x", "1.0.2": "a", "4.0.3": "b", "4.0.4": "c" },
            "_distfiles": {
                "balanced-match-4.0.3.tgz": { "url": "u" },
                "balanced-match-4.0.4.tgz": { "url": "u" }
            },
            "_uplinks": { "npmjs": { "fetched": 1 } }
        })
    }

    fn simple_manifest(name: &str, versions: &[&str], latest: &str) -> serde_json::Value {
        let versions: serde_json::Map<String, serde_json::Value> =
            versions.iter().map(|v| (v.to_string(), json!({}))).collect();
        json!({ "name": name, "versions": versions, "dist-tags": { "latest": latest } })
    }

    #[test]
    fn check_reports_versions_without_tarball() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(tmp.path(), "balanced-match", balanced_match_manifest(), &["1.0.2", "4.0.2"]);
        write_pkg(tmp.path(), "@scope/ok", simple_manifest("@scope/ok", &["1.0.0"], "1.0.0"), &["1.0.0"]);

        let report = check_storage(tmp.path()).unwrap();
        assert_eq!(report.scanned, 2);
        assert_eq!(report.unhealthy.len(), 1);
        let pkg = &report.unhealthy[0];
        assert_eq!(pkg.name, "balanced-match");
        assert_eq!(pkg.missing_versions, vec!["4.0.3", "4.0.4"]);
        assert!(pkg.latest_missing);
        assert!(pkg.latest_stale);
        assert_eq!(pkg.expected_latest.as_deref(), Some("4.0.2"));
    }

    #[test]
    fn check_reports_stale_latest_even_when_all_tarballs_exist() {
        // 4.0.4 已补包，但 latest 仍停在旧的 4.0.3：npm 对 ^4.0.2 会选 4.0.3
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(
            tmp.path(),
            "balanced-match",
            simple_manifest("balanced-match", &["4.0.3", "4.0.4"], "4.0.3"),
            &["4.0.3", "4.0.4"],
        );

        let report = check_storage(tmp.path()).unwrap();
        let pkg = &report.unhealthy[0];
        assert!(pkg.missing_versions.is_empty());
        assert!(!pkg.latest_missing);
        assert!(pkg.latest_stale);
        assert_eq!(pkg.expected_latest.as_deref(), Some("4.0.4"));
    }

    #[test]
    fn repair_prunes_phantom_versions_and_retargets_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_pkg(tmp.path(), "balanced-match", balanced_match_manifest(), &["1.0.2", "4.0.2"]);

        let outcomes = repair_packages(tmp.path(), &["balanced-match".to_string()], "http://r").unwrap();
        assert_eq!(outcomes[0].action, RepairAction::Repaired);
        assert_eq!(outcomes[0].removed_versions, vec!["4.0.3", "4.0.4"]);
        assert_eq!(outcomes[0].previous_latest.as_deref(), Some("4.0.3"));
        assert_eq!(outcomes[0].new_latest.as_deref(), Some("4.0.2"));
        assert!(Path::new(outcomes[0].backup_path.as_ref().unwrap()).exists());

        let json = read_manifest(&dir).unwrap();
        let versions: Vec<&String> = json["versions"].as_object().unwrap().keys().collect();
        assert_eq!(versions, vec!["1.0.2", "4.0.2"]);
        assert_eq!(json["dist-tags"]["latest"], "4.0.2");
        assert!(json["dist-tags"].get("next").is_none());
        assert!(json["time"].get("4.0.3").is_none());
        assert_eq!(json["time"]["1.0.2"], "a");
        assert!(json["_distfiles"].as_object().unwrap().is_empty());
        assert!(json.get("_uplinks").is_some());

        assert!(check_storage(tmp.path()).unwrap().unhealthy.is_empty());
    }

    #[test]
    fn repair_moves_stale_latest_forward() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_pkg(
            tmp.path(),
            "balanced-match",
            simple_manifest("balanced-match", &["4.0.3", "4.0.4", "5.0.0-rc.1"], "4.0.3"),
            &["4.0.3", "4.0.4", "5.0.0-rc.1"],
        );

        let outcomes = repair_packages(tmp.path(), &["balanced-match".to_string()], "http://r").unwrap();
        assert_eq!(outcomes[0].action, RepairAction::Repaired);
        assert!(outcomes[0].removed_versions.is_empty());
        assert_eq!(outcomes[0].new_latest.as_deref(), Some("4.0.4"));
        assert_eq!(read_manifest(&dir).unwrap()["dist-tags"]["latest"], "4.0.4");
    }

    #[test]
    fn repair_removes_manifest_when_no_tarball_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_pkg(tmp.path(), "@scope/ghost", simple_manifest("@scope/ghost", &["1.0.0"], "1.0.0"), &[]);

        let outcomes = repair_packages(tmp.path(), &["@scope/ghost".to_string()], "http://r").unwrap();
        assert_eq!(outcomes[0].action, RepairAction::Removed);
        assert!(!dir.join("package.json").exists());
        assert!(Path::new(outcomes[0].backup_path.as_ref().unwrap()).exists());
    }

    #[test]
    fn repair_skips_healthy_and_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(tmp.path(), "ok", simple_manifest("ok", &["1.0.0"], "1.0.0"), &["1.0.0"]);

        let outcomes =
            repair_packages(tmp.path(), &["ok".to_string(), "../evil".to_string()], "http://r").unwrap();
        assert_eq!(outcomes[0].action, RepairAction::Skipped);
        assert_eq!(outcomes[1].action, RepairAction::Failed);
    }

    #[test]
    fn assess_flags_package_without_any_tarball() {
        let pkg = assess("ghost".into(), vec!["1.0.0".into()], &[], Some("1.0.0".into())).unwrap();
        assert_eq!(pkg.missing_versions, vec!["1.0.0"]);
        assert!(pkg.cached_versions.is_empty());
        assert!(pkg.latest_missing);
        assert!(!pkg.latest_stale);
        assert!(assess("ok".into(), vec!["1.0.0".into()], &["1.0.0".into()], Some("1.0.0".into())).is_none());
    }

    #[test]
    fn orphan_tarball_is_reported_and_registered_locally() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_pkg(tmp.path(), "ansi-styles", simple_manifest("ansi-styles", &["3.2.1"], "3.2.1"), &["3.2.1"]);
        std::fs::write(
            dir.join("ansi-styles-4.3.0.tgz"),
            crate::orphan_repair::test_tarball("ansi-styles", "4.3.0"),
        )
        .unwrap();

        let report = check_storage(tmp.path()).unwrap();
        let pkg = &report.unhealthy[0];
        assert_eq!(pkg.orphan_versions, vec!["4.3.0"]);
        assert_eq!(pkg.expected_latest.as_deref(), Some("4.3.0"));

        let outcomes = repair_packages(tmp.path(), &["ansi-styles".to_string()], "http://r").unwrap();
        assert_eq!(outcomes[0].action, RepairAction::Repaired);
        assert_eq!(outcomes[0].registered_versions, vec!["4.3.0"]);
        assert_eq!(outcomes[0].new_latest.as_deref(), Some("4.3.0"));
        let json = read_manifest(&dir).unwrap();
        assert_eq!(json["versions"]["4.3.0"]["dependencies"]["dep"], "^1");
        assert_eq!(json["versions"]["4.3.0"]["dist"]["tarball"], "http://r/ansi-styles/-/ansi-styles-4.3.0.tgz");
        assert_eq!(json["_attachments"]["ansi-styles-4.3.0.tgz"]["version"], "4.3.0");
        assert!(check_storage(tmp.path()).unwrap().unhealthy.is_empty());
    }

    #[test]
    fn local_repair_refuses_orphan_with_mismatched_content() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_pkg(tmp.path(), "ansi-styles", simple_manifest("ansi-styles", &["3.2.1"], "3.2.1"), &["3.2.1"]);
        std::fs::write(dir.join("ansi-styles-4.3.0.tgz"), b"not a tarball").unwrap();

        let outcomes = repair_packages(tmp.path(), &["ansi-styles".to_string()], "http://r").unwrap();
        assert_eq!(outcomes[0].action, RepairAction::Failed);
        assert!(read_manifest(&dir).unwrap()["versions"].get("4.3.0").is_none());
    }

    #[test]
    fn preferred_latest_skips_prerelease() {
        let versions = vec!["1.0.0".to_string(), "2.0.0-beta.1".to_string()];
        assert_eq!(preferred_latest(&versions).as_deref(), Some("1.0.0"));
        let only_pre = vec!["2.0.0-beta.1".to_string()];
        assert_eq!(preferred_latest(&only_pre).as_deref(), Some("2.0.0-beta.1"));
    }
}
