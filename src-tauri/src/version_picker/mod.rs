//! 按 npm 客户端（npm-pick-manifest）的规则从 packument 中挑选版本，
//! 使预热结果与目标环境 `npm install` 实际安装的版本一致。

use node_semver::{Range, Version};
use std::collections::HashMap;

/// 版本挑选所需的单个版本元数据，取自 packument 的 `versions[ver]`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionMeta {
    pub deprecated: bool,
    pub engines_node: Option<String>,
}

impl VersionMeta {
    pub fn from_manifest(manifest: &serde_json::Value) -> Self {
        // npm 以 truthy 判断 deprecated：空字符串不算废弃
        let deprecated = match manifest.get("deprecated") {
            Some(serde_json::Value::String(s)) => !s.is_empty(),
            Some(serde_json::Value::Bool(b)) => *b,
            _ => false,
        };
        // 早期包的 engines 可能是数组，npm 视为未声明 node 约束
        let engines_node = manifest
            .get("engines")
            .and_then(|e| e.get("node"))
            .and_then(|n| n.as_str())
            .map(str::to_string);
        Self {
            deprecated,
            engines_node,
        }
    }
}

/// 从 packument 的 `versions` 对象批量提取元数据。
pub fn metas_from_versions(
    versions: &serde_json::Map<String, serde_json::Value>,
) -> HashMap<String, VersionMeta> {
    versions
        .iter()
        .map(|(v, m)| (v.clone(), VersionMeta::from_manifest(m)))
        .collect()
}

/// 未提供 Node 版本或包未声明 engines.node 时视为兼容。
fn engine_ok(meta: &VersionMeta, node: Option<&Version>) -> bool {
    let (Some(node), Some(engines)) = (node, meta.engines_node.as_deref()) else {
        return true;
    };
    // node-semver 无法解析的历史写法按兼容处理，避免误把版本降级
    Range::parse(engines).map_or(true, |r| r.satisfies(node))
}

/// 把精确版本 / semver range / dist-tag 解析为具体版本：
/// 1. 精确版本或 dist-tag 直接命中；
/// 2. `latest` 满足 range、未废弃且 engines 兼容时优先选它；
/// 3. 否则按「未废弃且 engines 兼容 → engines 兼容 → 未废弃 → 版本号」排序取最优。
///
/// `node` 为目标 Node 版本，None 时不考虑 engines。无满足的版本或未知
/// dist-tag 时返回 None。
pub fn pick_version(
    spec: &str,
    versions: &HashMap<String, VersionMeta>,
    dist_tags: &HashMap<String, String>,
    node: Option<&Version>,
) -> Option<String> {
    let spec = match spec.trim() {
        "" => "*",
        s => s,
    };
    if versions.contains_key(spec) {
        return Some(spec.to_string());
    }
    // 与 npm-package-arg 一致：合法 range 优先，其余视为 dist-tag
    let Ok(range) = Range::parse(spec) else {
        return dist_tags.get(spec).cloned();
    };

    if let Some(latest) = dist_tags.get("latest") {
        if let Some(meta) = versions.get(latest) {
            // 与 npm 一致：`*` 直接接受 latest（即便是预发布版本）
            let in_range = spec == "*"
                || Version::parse(latest).map_or(false, |v| range.satisfies(&v));
            if in_range && !meta.deprecated && engine_ok(meta, node) {
                return Some(latest.clone());
            }
        }
    }

    let rank = |meta: &VersionMeta| {
        let engine = engine_ok(meta, node);
        let not_deprecated = !meta.deprecated;
        (not_deprecated && engine, engine, not_deprecated)
    };
    versions
        .iter()
        .filter_map(|(v, meta)| {
            let ver = Version::parse(v).ok()?;
            range.satisfies(&ver).then_some((ver, v, meta))
        })
        .max_by(|(va, _, ma), (vb, _, mb)| rank(ma).cmp(&rank(mb)).then_with(|| va.cmp(vb)))
        .map(|(_, v, _)| v.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(deprecated: bool, engines_node: Option<&str>) -> VersionMeta {
        VersionMeta {
            deprecated,
            engines_node: engines_node.map(str::to_string),
        }
    }

    fn versions(entries: &[(&str, VersionMeta)]) -> HashMap<String, VersionMeta> {
        entries
            .iter()
            .map(|(v, m)| (v.to_string(), m.clone()))
            .collect()
    }

    fn tags(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn node(v: &str) -> Version {
        Version::parse(v).unwrap()
    }

    #[test]
    fn picks_highest_without_node_constraint() {
        let vs = versions(&[
            ("1.0.0", meta(false, None)),
            ("1.2.0", meta(false, Some(">=20"))),
        ]);
        assert_eq!(pick_version("^1.0.0", &vs, &tags(&[]), None).as_deref(), Some("1.2.0"));
    }

    #[test]
    fn skips_engine_incompatible_versions() {
        let vs = versions(&[
            ("1.0.0", meta(false, Some(">=14"))),
            ("1.1.0", meta(false, Some(">=16"))),
            ("1.2.0", meta(false, Some(">=20"))),
        ]);
        let n = node("18.20.4");
        assert_eq!(pick_version("^1.0.0", &vs, &tags(&[]), Some(&n)).as_deref(), Some("1.1.0"));
    }

    #[test]
    fn falls_back_to_highest_when_no_engine_match() {
        let vs = versions(&[
            ("1.0.0", meta(false, Some(">=20"))),
            ("1.1.0", meta(false, Some(">=22"))),
        ]);
        let n = node("16.0.0");
        assert_eq!(pick_version("^1.0.0", &vs, &tags(&[]), Some(&n)).as_deref(), Some("1.1.0"));
    }

    #[test]
    fn skips_deprecated_versions() {
        let vs = versions(&[
            ("1.0.0", meta(false, None)),
            ("1.1.0", meta(true, None)),
        ]);
        assert_eq!(pick_version("^1.0.0", &vs, &tags(&[]), None).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn prefers_engine_ok_and_not_deprecated_over_engine_ok_only() {
        let vs = versions(&[
            ("1.0.0", meta(false, Some(">=20"))),
            ("1.1.0", meta(true, Some(">=14"))),
            ("1.2.0", meta(false, Some(">=14"))),
            ("1.3.0", meta(true, Some(">=14"))),
        ]);
        let n = node("18.0.0");
        assert_eq!(pick_version("^1.0.0", &vs, &tags(&[]), Some(&n)).as_deref(), Some("1.2.0"));
    }

    #[test]
    fn prefers_latest_tag_within_range() {
        let vs = versions(&[
            ("1.0.0", meta(false, None)),
            ("1.5.0", meta(false, None)),
        ]);
        let t = tags(&[("latest", "1.0.0")]);
        assert_eq!(pick_version("^1.0.0", &vs, &t, None).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn ignores_latest_when_engine_incompatible() {
        let vs = versions(&[
            ("1.0.0", meta(false, Some(">=14"))),
            ("1.5.0", meta(false, Some(">=20"))),
        ]);
        let t = tags(&[("latest", "1.5.0")]);
        let n = node("18.0.0");
        assert_eq!(pick_version("^1.0.0", &vs, &t, Some(&n)).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn ignores_latest_outside_range() {
        let vs = versions(&[
            ("1.0.0", meta(false, None)),
            ("2.0.0", meta(false, None)),
        ]);
        let t = tags(&[("latest", "2.0.0")]);
        assert_eq!(pick_version("^1.0.0", &vs, &t, None).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn star_accepts_prerelease_latest() {
        let vs = versions(&[
            ("1.0.0", meta(false, None)),
            ("2.0.0-rc.1", meta(false, None)),
        ]);
        let t = tags(&[("latest", "2.0.0-rc.1")]);
        assert_eq!(pick_version("*", &vs, &t, None).as_deref(), Some("2.0.0-rc.1"));
        assert_eq!(pick_version("", &vs, &t, None).as_deref(), Some("2.0.0-rc.1"));
    }

    #[test]
    fn range_excludes_prereleases() {
        let vs = versions(&[
            ("1.0.0", meta(false, None)),
            ("1.1.0-beta.1", meta(false, None)),
        ]);
        assert_eq!(pick_version("^1.0.0", &vs, &tags(&[]), None).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn exact_version_and_tag_bypass_heuristics() {
        let vs = versions(&[
            ("1.0.0", meta(true, Some(">=99"))),
            ("1.1.0", meta(false, None)),
        ]);
        let t = tags(&[("next", "1.0.0")]);
        let n = node("18.0.0");
        assert_eq!(pick_version("1.0.0", &vs, &t, Some(&n)).as_deref(), Some("1.0.0"));
        assert_eq!(pick_version("next", &vs, &t, Some(&n)).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn unparseable_engines_treated_as_compatible() {
        let vs = versions(&[
            ("1.0.0", meta(false, None)),
            ("1.1.0", meta(false, Some("not a range!!"))),
        ]);
        let n = node("18.0.0");
        assert_eq!(pick_version("^1.0.0", &vs, &tags(&[]), Some(&n)).as_deref(), Some("1.1.0"));
    }

    #[test]
    fn returns_none_for_invalid_or_unsatisfied_range() {
        let vs = versions(&[("1.0.0", meta(false, None))]);
        assert_eq!(pick_version("git+ssh://x", &vs, &tags(&[]), None), None);
        assert_eq!(pick_version("^2.0.0", &vs, &tags(&[]), None), None);
    }

    #[test]
    fn version_meta_from_manifest() {
        let m = VersionMeta::from_manifest(&serde_json::json!({
            "deprecated": "use v2",
            "engines": { "node": ">=18" }
        }));
        assert_eq!(m, meta(true, Some(">=18")));

        let m = VersionMeta::from_manifest(&serde_json::json!({
            "deprecated": "",
            "engines": ["node >=0.4"]
        }));
        assert_eq!(m, meta(false, None));
    }
}
