use crate::parser::ParsedDependency;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplenishmentEntry {
    pub name: String,
    pub version: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplenishmentList {
    pub kind: String,
    pub schema_version: u32,
    /// ISO 8601 UTC 时间，由生成清单的前端提供。
    pub generated_at: String,
    pub source_registry: String,
    pub entries: Vec<ReplenishmentEntry>,
}

pub fn validate_package(name: &str, version: &str) -> Result<(), String> {
    let valid_part = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.~".contains(c))
            && s != "."
            && s != ".."
    };
    let valid_name = if let Some(scoped) = name.strip_prefix('@') {
        scoped
            .split_once('/')
            .is_some_and(|(scope, package)| valid_part(scope) && valid_part(package))
    } else {
        valid_part(name)
    };
    if !valid_name || crate::parser::pinned_version(version).as_deref() != Some(version) {
        return Err(format!(
            "补包条目必须是有效包名和精确版本: {}@{}",
            name, version
        ));
    }
    Ok(())
}

impl ReplenishmentList {
    pub fn validate(&self) -> Result<(), String> {
        if self.kind != "verdaccio-replenishment" || self.schema_version != 1 {
            return Err("不支持的补包清单格式或 schema_version".into());
        }
        if self.generated_at.trim().is_empty() || self.source_registry.trim().is_empty() {
            return Err("补包清单缺少生成时间或来源仓库".into());
        }
        let mut seen = HashSet::new();
        for entry in &self.entries {
            validate_package(&entry.name, &entry.version)?;
            if entry.reason.trim().is_empty() || !seen.insert((&entry.name, &entry.version)) {
                return Err(format!(
                    "补包条目原因为空或重复: {}@{}",
                    entry.name, entry.version
                ));
            }
        }
        Ok(())
    }

    pub fn dependencies(&self) -> Vec<ParsedDependency> {
        self.entries
            .iter()
            .map(|e| ParsedDependency {
                name: e.name.clone(),
                version: e.version.clone(),
                tarball_url: None,
            })
            .collect()
    }
}

pub fn read_list(path: &Path) -> Result<ReplenishmentList, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("读取补包清单失败: {}", e))?;
    let list: ReplenishmentList =
        serde_json::from_str(&content).map_err(|e| format!("解析补包清单失败: {}", e))?;
    list.validate()?;
    Ok(list)
}

pub fn write_list(path: &Path, list: &ReplenishmentList) -> Result<(), String> {
    list.validate()?;
    std::fs::write(
        path,
        serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct DependencyFile {
    pub kind: String,
    pub dependencies: Vec<ParsedDependency>,
    pub replenishment: Option<ReplenishmentList>,
}

pub fn read_dependency_file(path: &Path) -> Result<DependencyFile, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let json: Option<serde_json::Value> = serde_json::from_str(&content).ok();
    if json.as_ref().is_some_and(|v| {
        v["kind"].as_str() == Some("verdaccio-replenishment")
            || (v.get("entries").is_some() && v.get("schema_version").is_some())
    })
    {
        let list = read_list(path)?;
        return Ok(DependencyFile {
            kind: "replenishment".into(),
            dependencies: list.dependencies(),
            replenishment: Some(list),
        });
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let kind = if matches!(name, "pnpm-lock.yaml" | "package-lock.json") {
        "lockfile"
    } else {
        "manifest"
    };
    Ok(DependencyFile {
        kind: kind.into(),
        dependencies: crate::parser::detect_and_parse(path)?,
        replenishment: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn list() -> ReplenishmentList {
        ReplenishmentList {
            kind: "verdaccio-replenishment".into(),
            schema_version: 1,
            generated_at: "2026-10-19T00:00:00.000Z".into(),
            source_registry: "http://intranet:4873".into(),
            entries: vec![ReplenishmentEntry {
                name: "@scope/pkg".into(),
                version: "1.2.3".into(),
                reason: "missing_tarball".into(),
            }],
        }
    }
    #[test]
    fn roundtrip_arbitrary_filename_keeps_exact_closure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repair.json");
        write_list(&path, &list()).unwrap();
        let file = read_dependency_file(&path).unwrap();
        assert_eq!(file.kind, "replenishment");
        assert_eq!(file.dependencies[0].version, "1.2.3");
        assert_eq!(file.dependencies.len(), 1);
    }
    #[test]
    fn rejects_ranges_unknown_schema_and_unsafe_names() {
        for (name, version) in [("a", "^1"), ("../a", "1.0.0"), ("@s/../a", "1.0.0")] {
            assert!(validate_package(name, version).is_err());
        }
        let mut invalid = list();
        invalid.schema_version = 2;
        assert!(invalid.validate().is_err());
        let mut invalid = list();
        invalid.entries.push(invalid.entries[0].clone());
        assert!(invalid.validate().is_err());
    }
}
