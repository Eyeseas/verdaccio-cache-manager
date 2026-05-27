use node_semver::{Range, Version};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const SUPPORTED_SECTIONS: [&str; 3] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DowngradeStatus {
    UnchangedCached,
    RewrittenCached,
    Downgraded,
    MajorDowngraded,
    MissingCache,
    UnsupportedSpec,
    InvalidRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DowngradeItem {
    pub name: String,
    pub section: String,
    pub original_spec: String,
    pub original_resolved_version: Option<String>,
    pub target_version: Option<String>,
    pub cached_versions: Vec<String>,
    pub status: DowngradeStatus,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DowngradeSummary {
    pub total: usize,
    pub changed: usize,
    pub rewritten_cached: usize,
    pub unchanged_cached: usize,
    pub missing_cache: usize,
    pub unsupported: usize,
    pub invalid: usize,
    pub major_downgraded: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DowngradeAnalysis {
    pub request_id: Option<String>,
    pub file_path: String,
    pub file_name: String,
    pub allow_major_downgrade: bool,
    pub original_content: String,
    pub updated_content: String,
    pub items: Vec<DowngradeItem>,
    pub summary: DowngradeSummary,
    pub cache_index_empty: bool,
}

pub fn analyze_content(
    file_name: &str,
    content: &str,
    cached_versions: &HashMap<String, Vec<String>>,
    allow_major_downgrade: bool,
    request_id: Option<String>,
) -> Result<DowngradeAnalysis, String> {
    if !is_supported_package_file(file_name) {
        return Err("仅支持 package.json".to_string());
    }

    let mut json: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("JSON 解析失败: {}", e))?;

    let mut items = Vec::new();
    for section in SUPPORTED_SECTIONS {
        let Some(obj) = json.get_mut(section).and_then(|v| v.as_object_mut()) else {
            continue;
        };

        for (name, value) in obj.iter_mut() {
            let Some(spec) = value.as_str() else {
                continue;
            };
            let item = analyze_dependency(
                name,
                section,
                spec,
                cached_versions
                    .get(name)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                allow_major_downgrade,
            );
            if let Some(target) = &item.target_version {
                if is_changed_status(&item.status) {
                    *value = serde_json::Value::String(target.clone());
                }
            }
            items.push(item);
        }
    }

    let updated_content = serde_json::to_string_pretty(&json)
        .map_err(|e| format!("生成 package.json 失败: {}", e))?;
    let summary = recompute_summary(&items);

    Ok(DowngradeAnalysis {
        request_id,
        file_path: file_name.to_string(),
        file_name: file_name.to_string(),
        allow_major_downgrade,
        original_content: content.to_string(),
        updated_content,
        items,
        summary,
        cache_index_empty: cached_versions.is_empty(),
    })
}

fn is_supported_package_file(file_name: &str) -> bool {
    Path::new(file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "package.json")
}

fn analyze_dependency(
    name: &str,
    section: &str,
    original_spec: &str,
    cached_versions: &[String],
    allow_major_downgrade: bool,
) -> DowngradeItem {
    let spec = original_spec.trim();
    let mut sorted_cached = sorted_versions(cached_versions);
    let cached_strings: Vec<String> = sorted_cached.iter().map(ToString::to_string).collect();
    let original_exact = Version::parse(spec).ok();

    let mut base = DowngradeItem {
        name: name.to_string(),
        section: section.to_string(),
        original_spec: original_spec.to_string(),
        original_resolved_version: original_exact.as_ref().map(ToString::to_string),
        target_version: None,
        cached_versions: cached_strings,
        status: DowngradeStatus::MissingCache,
        reason: String::new(),
    };

    if let Some(reason) = unsupported_spec_reason(spec) {
        base.status = DowngradeStatus::UnsupportedSpec;
        base.reason = reason;
        return base;
    }

    if sorted_cached.is_empty() {
        base.status = DowngradeStatus::MissingCache;
        base.reason = "没有缓存版本".to_string();
        return base;
    }

    sorted_cached.sort();

    if let Some(target) = highest_satisfying(spec, &sorted_cached) {
        base.target_version = Some(target.to_string());
        base.status = status_for_target(spec, original_exact.as_ref(), &target);
        base.reason = reason_for_status(&base.status);
        return base;
    }

    if Range::parse(spec).is_err() && original_exact.is_none() {
        base.status = DowngradeStatus::InvalidRange;
        base.reason = "版本范围无法解析".to_string();
        return base;
    }

    let target = if allow_major_downgrade {
        sorted_cached.last().cloned()
    } else {
        infer_major(spec).and_then(|major| {
            sorted_cached
                .iter()
                .filter(|v| v.major == major)
                .last()
                .cloned()
        })
    };

    let Some(target) = target else {
        base.status = DowngradeStatus::MissingCache;
        base.reason = "没有符合策略的缓存版本".to_string();
        return base;
    };

    base.target_version = Some(target.to_string());
    base.status = fallback_status(original_exact.as_ref(), &target, allow_major_downgrade);
    base.reason = reason_for_status(&base.status);
    base
}

fn sorted_versions(versions: &[String]) -> Vec<Version> {
    let mut parsed: Vec<Version> = versions
        .iter()
        .filter_map(|v| Version::parse(v).ok())
        .collect();
    parsed.sort();
    parsed
}

fn highest_satisfying(spec: &str, versions: &[Version]) -> Option<Version> {
    let range = Range::parse(spec).ok()?;
    versions
        .iter()
        .filter(|v| range.satisfies(v))
        .last()
        .cloned()
}

fn infer_major(spec: &str) -> Option<u64> {
    if let Ok(version) = Version::parse(spec) {
        return Some(version.major);
    }

    let digits: String = spec
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn unsupported_spec_reason(spec: &str) -> Option<String> {
    if spec.is_empty() || spec == "*" || spec.eq_ignore_ascii_case("latest") {
        return Some("不支持 latest、* 或空版本".to_string());
    }

    let lower = spec.to_ascii_lowercase();
    for prefix in [
        "file:",
        "workspace:",
        "git:",
        "git+",
        "git@",
        "http://",
        "https://",
        "link:",
        "npm:",
    ] {
        if lower.starts_with(prefix) {
            return Some(format!("不支持 {} 版本来源", prefix.trim_end_matches(':')));
        }
    }

    None
}

fn status_for_target(
    spec: &str,
    original_exact: Option<&Version>,
    target: &Version,
) -> DowngradeStatus {
    if let Some(original) = original_exact {
        if original == target {
            return DowngradeStatus::UnchangedCached;
        }
        if original.major != target.major {
            return DowngradeStatus::MajorDowngraded;
        }
        if target < original {
            return DowngradeStatus::Downgraded;
        }
        return DowngradeStatus::RewrittenCached;
    }

    if infer_major(spec).is_some_and(|major| major != target.major) {
        DowngradeStatus::MajorDowngraded
    } else {
        DowngradeStatus::RewrittenCached
    }
}

fn fallback_status(
    original_exact: Option<&Version>,
    target: &Version,
    allow_major_downgrade: bool,
) -> DowngradeStatus {
    if allow_major_downgrade
        && original_exact
            .map(|original| original.major != target.major)
            .unwrap_or(true)
    {
        return DowngradeStatus::MajorDowngraded;
    }

    if let Some(original) = original_exact {
        if original == target {
            DowngradeStatus::UnchangedCached
        } else if original.major != target.major {
            DowngradeStatus::MajorDowngraded
        } else if target < original {
            DowngradeStatus::Downgraded
        } else {
            DowngradeStatus::RewrittenCached
        }
    } else {
        DowngradeStatus::Downgraded
    }
}

fn reason_for_status(status: &DowngradeStatus) -> String {
    match status {
        DowngradeStatus::UnchangedCached => "原版本已缓存".to_string(),
        DowngradeStatus::RewrittenCached => "改写为已缓存精确版本".to_string(),
        DowngradeStatus::Downgraded => "降级到同 major 已缓存版本".to_string(),
        DowngradeStatus::MajorDowngraded => "跨 major 使用已缓存版本".to_string(),
        DowngradeStatus::MissingCache => "没有可用缓存版本".to_string(),
        DowngradeStatus::UnsupportedSpec => "不支持的版本声明".to_string(),
        DowngradeStatus::InvalidRange => "版本范围无法解析".to_string(),
    }
}

fn is_changed_status(status: &DowngradeStatus) -> bool {
    matches!(
        status,
        DowngradeStatus::RewrittenCached
            | DowngradeStatus::Downgraded
            | DowngradeStatus::MajorDowngraded
    )
}

fn recompute_summary(items: &[DowngradeItem]) -> DowngradeSummary {
    let mut summary = DowngradeSummary {
        total: items.len(),
        ..Default::default()
    };

    for item in items {
        match item.status {
            DowngradeStatus::UnchangedCached => summary.unchanged_cached += 1,
            DowngradeStatus::RewrittenCached => {
                summary.changed += 1;
                summary.rewritten_cached += 1;
            }
            DowngradeStatus::Downgraded => summary.changed += 1,
            DowngradeStatus::MajorDowngraded => {
                summary.changed += 1;
                summary.major_downgraded += 1;
            }
            DowngradeStatus::MissingCache => summary.missing_cache += 1,
            DowngradeStatus::UnsupportedSpec => summary.unsupported += 1,
            DowngradeStatus::InvalidRange => summary.invalid += 1,
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn same_major_fallback_downgrades_to_highest_cached_same_major() {
        let mut cached = HashMap::new();
        cached.insert(
            "react".to_string(),
            vec!["18.2.0".to_string(), "18.3.0".to_string()],
        );
        let input = r#"{"dependencies":{"react":"18.4.0"}}"#;

        let analysis = analyze_content("package.json", input, &cached, false, None).unwrap();

        assert_eq!(analysis.summary.changed, 1);
        assert_eq!(analysis.items[0].status, DowngradeStatus::Downgraded);
        assert_eq!(analysis.items[0].target_version.as_deref(), Some("18.3.0"));
        assert!(analysis.updated_content.contains("\"react\": \"18.3.0\""));
    }

    #[test]
    fn cross_major_fallback_uses_highest_cached_when_enabled() {
        let mut cached = HashMap::new();
        cached.insert("react".to_string(), vec!["17.9.0".to_string()]);
        let input = r#"{"dependencies":{"react":"^18.4.0"}}"#;

        let analysis = analyze_content("package.json", input, &cached, true, None).unwrap();

        assert_eq!(analysis.items[0].status, DowngradeStatus::MajorDowngraded);
        assert_eq!(analysis.items[0].target_version.as_deref(), Some("17.9.0"));
    }

    #[test]
    fn range_satisfying_cached_version_is_rewritten_cached() {
        let mut cached = HashMap::new();
        cached.insert("vite".to_string(), vec!["7.0.4".to_string()]);
        let input = r#"{"devDependencies":{"vite":"^7.0.0"}}"#;

        let analysis = analyze_content("package.json", input, &cached, false, None).unwrap();

        assert_eq!(analysis.items[0].status, DowngradeStatus::RewrittenCached);
        assert!(analysis.updated_content.contains("\"vite\": \"7.0.4\""));
    }

    #[test]
    fn unsupported_specs_and_unsupported_sections_remain_unchanged() {
        let mut cached = HashMap::new();
        cached.insert("local-lib".to_string(), vec!["1.0.0".to_string()]);
        cached.insert("peer-lib".to_string(), vec!["1.0.0".to_string()]);
        let input = r#"{
          "dependencies":{"local-lib":"file:../local-lib"},
          "peerDependencies":{"peer-lib":"2.0.0"},
          "overrides":{"peer-lib":"2.0.0"}
        }"#;

        let analysis = analyze_content("package.json", input, &cached, false, None).unwrap();

        assert_eq!(analysis.items[0].status, DowngradeStatus::UnsupportedSpec);
        assert!(
            analysis
                .updated_content
                .contains("\"local-lib\": \"file:../local-lib\"")
        );
        assert!(analysis.updated_content.contains("\"peer-lib\": \"2.0.0\""));
    }

    #[test]
    fn missing_cache_keeps_original_spec() {
        let cached = HashMap::new();
        let input = r#"{"optionalDependencies":{"fsevents":"^2.3.3"}}"#;

        let analysis = analyze_content("package.json", input, &cached, false, None).unwrap();

        assert_eq!(analysis.items[0].status, DowngradeStatus::MissingCache);
        assert!(analysis.updated_content.contains("\"fsevents\": \"^2.3.3\""));
    }
}
