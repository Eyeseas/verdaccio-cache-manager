use crate::registry_client::SearchResult;
use std::path::Path;

pub fn scan_storage(storage_path: &Path) -> Result<Vec<SearchResult>, String> {
    if !storage_path.exists() {
        return Err(format!(
            "storage 目录不存在: {}",
            storage_path.display()
        ));
    }
    if !storage_path.is_dir() {
        return Err(format!(
            "storage 路径不是目录: {}",
            storage_path.display()
        ));
    }

    let mut results = Vec::new();
    let entries = std::fs::read_dir(storage_path).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if name.starts_with('@') {
            let sub_entries = match std::fs::read_dir(&path) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for sub in sub_entries.flatten() {
                let sub_path = sub.path();
                if sub_path.is_dir() {
                    if let Some(pkg) = read_package_manifest(&sub_path) {
                        results.push(pkg);
                    }
                }
            }
        } else if let Some(pkg) = read_package_manifest(&path) {
            results.push(pkg);
        }
    }

    results.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(results)
}

fn read_package_manifest(pkg_dir: &Path) -> Option<SearchResult> {
    let manifest = pkg_dir.join("package.json");
    if !manifest.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&manifest).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let name = json["name"].as_str()?.to_string();

    let cached_versions = list_cached_versions(pkg_dir, &name);
    if cached_versions.is_empty() {
        return None;
    }

    let mut versions: Vec<String> = json["versions"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    versions.sort_by(|a, b| compare_versions(a, b));

    let dist_tag_latest = json["dist-tags"]["latest"]
        .as_str()
        .map(|s| s.to_string());

    let latest = dist_tag_latest
        .clone()
        .or_else(|| versions.last().cloned())
        .or_else(|| cached_versions.last().cloned());

    let description = json["description"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| {
            latest.as_ref().and_then(|v| {
                json["versions"][v]["description"]
                    .as_str()
                    .map(|s| s.to_string())
            })
        });

    Some(SearchResult {
        name,
        description,
        latest_version: latest,
        versions,
        cached_versions,
    })
}

fn list_cached_versions(pkg_dir: &Path, pkg_name: &str) -> Vec<String> {
    let name_part = pkg_name.rsplit('/').next().unwrap_or(pkg_name);
    let prefix = format!("{}-", name_part);

    let entries = match std::fs::read_dir(pkg_dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut versions: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let filename = entry.file_name().to_string_lossy().to_string();
            let stripped = filename.strip_prefix(&prefix)?;
            let version = stripped.strip_suffix(".tgz")?;
            Some(version.to_string())
        })
        .collect();

    versions.sort_by(|a, b| compare_versions(a, b));
    versions
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u32> {
        s.split(|c: char| c == '.' || c == '-')
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect()
    };
    let pa = parse(a);
    let pb = parse(b);
    for i in 0..pa.len().max(pb.len()) {
        let na = pa.get(i).copied().unwrap_or(0);
        let nb = pb.get(i).copied().unwrap_or(0);
        match na.cmp(&nb) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}
