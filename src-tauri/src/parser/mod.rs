use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedDependency {
    pub name: String,
    pub version: String,
    pub tarball_url: Option<String>,
}

pub fn parse_package_json(content: &str) -> Result<Vec<ParsedDependency>, String> {
    let json: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("JSON 解析失败: {}", e))?;

    // 按名去重为单行。dependencies / devDependencies 之间「首次保留」
    // （dependencies 先遍历 → prod range 胜出，不静默丢弃 prod）；
    // 仅 optionalDependencies 按 npm 语义覆盖同名项。
    let mut deps: Vec<ParsedDependency> = Vec::new();
    let mut idx: HashMap<String, usize> = HashMap::new();

    for field in ["dependencies", "devDependencies"] {
        if let Some(obj) = json[field].as_object() {
            for (name, version) in obj {
                if idx.contains_key(name) {
                    continue;
                }
                if let Some(v) = version.as_str() {
                    if let Some(clean) = clean_semver_range(v) {
                        idx.insert(name.clone(), deps.len());
                        deps.push(ParsedDependency {
                            name: name.clone(),
                            version: clean,
                            tarball_url: None,
                        });
                    }
                }
            }
        }
    }

    if let Some(obj) = json["optionalDependencies"].as_object() {
        for (name, version) in obj {
            let existing_index = idx.remove(name);
            if let Some(i) = existing_index {
                deps.remove(i);
                for index in idx.values_mut() {
                    if *index > i {
                        *index -= 1;
                    }
                }
            }

            if let Some(v) = version.as_str() {
                if let Some(clean) = clean_semver_range(v) {
                    let entry = ParsedDependency {
                        name: name.clone(),
                        version: clean,
                        tarball_url: None,
                    };
                    idx.insert(name.clone(), deps.len());
                    deps.push(entry);
                }
            }
        }
    }

    Ok(deps)
}

fn clean_semver_range(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "*" || trimmed == "latest" {
        return None;
    }
    // Reject non-semver specifiers (git/file/url/workspace/link/npm alias)
    let lower = trimmed.to_ascii_lowercase();
    for proto in [
        "git+",
        "git:",
        "git@",
        "http://",
        "https://",
        "file:",
        "link:",
        "workspace:",
        "npm:",
    ] {
        if lower.starts_with(proto) {
            return None;
        }
    }
    // Keep raw range intact (e.g. "^9", "~5.1", ">=1.0.0 <2.0.0", "1.2.3").
    // Concrete pinning vs. range resolution is decided in resolve_version_ranges.
    Some(trimmed.to_string())
}

/// Returns Some(version) when the input is already a concrete x.y.z version
/// that doesn't need registry lookup.
pub fn pinned_version(raw: &str) -> Option<String> {
    let v = node_semver::Version::parse(raw).ok()?;
    Some(v.to_string())
}

pub fn parse_pnpm_lock(content: &str) -> Result<Vec<ParsedDependency>, String> {
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| format!("YAML 解析失败: {}", e))?;

    let mut deps = Vec::new();
    let mut seen: HashMap<String, bool> = HashMap::new();

    // Try "packages" field (all lockfile versions)
    if let Some(packages) = yaml["packages"].as_mapping() {
        for (key, value) in packages {
            if let Some(key_str) = key.as_str() {
                if let Some((name, version)) = parse_pnpm_package_key(key_str) {
                    // 过滤 git/file/link 等非 registry 条目（版本恒为精确 semver）
                    if pinned_version(&version).is_none() {
                        continue;
                    }
                    let dep_key = format!("{}@{}", name, version);
                    if seen.contains_key(&dep_key) {
                        continue;
                    }
                    seen.insert(dep_key, true);

                    let tarball_url = value["resolution"]["tarball"]
                        .as_str()
                        .map(|s| s.to_string());
                    deps.push(ParsedDependency {
                        name,
                        version,
                        tarball_url,
                    });
                }
            }
        }
    }

    // v9: also check "snapshots" if packages was empty
    if deps.is_empty() {
        if let Some(snapshots) = yaml["snapshots"].as_mapping() {
            for (key, _) in snapshots {
                if let Some(key_str) = key.as_str() {
                    if let Some((name, version)) = parse_pnpm_package_key(key_str) {
                        // 同 packages 循环：过滤非 registry 条目
                        if pinned_version(&version).is_none() {
                            continue;
                        }
                        let dep_key = format!("{}@{}", name, version);
                        if seen.contains_key(&dep_key) {
                            continue;
                        }
                        seen.insert(dep_key, true);
                        deps.push(ParsedDependency {
                            name,
                            version,
                            tarball_url: None,
                        });
                    }
                }
            }
        }
    }

    Ok(deps)
}

fn parse_pnpm_package_key(key: &str) -> Option<(String, String)> {
    // Strip optional leading '/'
    let key = key.strip_prefix('/').unwrap_or(key);
    // 先剥离括号 peer 后缀（v6/v9 如 "name@1.2.3(react@18.2.0)"），
    // 避免后续 rfind('@') 误命中括号内的 '@'
    let key = key.split('(').next().unwrap_or(key);

    // Try '@' separator first (v6+/v9 format: "name@version" or "@scope/name@version")
    // For scoped packages, we need the LAST '@' that isn't at position 0
    if let Some(at_pos) = find_version_separator(key) {
        let name = &key[..at_pos];
        let version = key[at_pos + 1..].trim();
        // 包名合法性校验：v5 peer 后缀 key（"/foo/1.2.3_react@16.14.0"）会被
        // '@' 分支误切出含 '/' 的名字，此时回落 v5 分支处理
        if is_valid_package_name(name) && !version.is_empty() {
            return Some((name.to_string(), version.to_string()));
        }
    }

    // Try v5 format: "name/version" or "@scope/name/version"
    if let Some((name, version)) = parse_v5_key(key) {
        return Some((name, version));
    }

    None
}

/// npm 包名合法性：scoped 名恰含一个 '/'（两段非空），非 scoped 名不含 '/'；
/// 均不得含 ':' 或空白。
fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() || name.contains(':') || name.contains(char::is_whitespace) {
        return false;
    }
    match name.strip_prefix('@') {
        Some(rest) => {
            let mut parts = rest.splitn(2, '/');
            matches!(
                (parts.next(), parts.next()),
                (Some(scope), Some(pkg))
                    if !scope.is_empty() && !pkg.is_empty() && !pkg.contains('/')
            )
        }
        None => !name.contains('/'),
    }
}

fn find_version_separator(key: &str) -> Option<usize> {
    // For scoped packages like "@scope/name@version", skip the first '@'
    let search_start = if key.starts_with('@') {
        key.find('/').unwrap_or(0) + 1
    } else {
        0
    };
    key[search_start..].rfind('@').map(|pos| pos + search_start)
}

fn parse_v5_key(key: &str) -> Option<(String, String)> {
    // v5 format: "package-name/1.2.3" or "@scope/package-name/1.2.3"
    // 版本可带 '_' peer 后缀（如 "1.2.3_react@16.14.0"），需剥离
    if key.starts_with('@') {
        // @scope/name/version - find the second '/'
        let after_scope = key.find('/')? + 1;
        let version_slash = key[after_scope..].find('/')?;
        let name = &key[..after_scope + version_slash];
        let version = key[after_scope + version_slash + 1..]
            .split('_')
            .next()
            .unwrap_or_default();
        if !version.is_empty() && version.chars().next()?.is_ascii_digit() {
            return Some((name.to_string(), version.to_string()));
        }
    } else {
        // name/version - find the last '/' where what follows starts with a digit
        if let Some(slash_pos) = key.rfind('/') {
            let name = &key[..slash_pos];
            let version = key[slash_pos + 1..].split('_').next().unwrap_or_default();
            if !name.is_empty() && !version.is_empty() && version.chars().next()?.is_ascii_digit() {
                return Some((name.to_string(), version.to_string()));
            }
        }
    }
    None
}

pub fn parse_package_lock(content: &str) -> Result<Vec<ParsedDependency>, String> {
    let json: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("JSON 解析失败: {}", e))?;

    let mut deps = Vec::new();
    let mut seen: HashMap<String, bool> = HashMap::new();

    if let Some(packages) = json["packages"].as_object() {
        for (key, value) in packages {
            if key.is_empty() {
                continue;
            }
            // workspace 软链条目指向本地目录,无 registry 产物
            if value["link"].as_bool() == Some(true) {
                continue;
            }
            // 路径不含 node_modules/ 的是 workspace 源目录(如 "packages/foo"),跳过
            let path_name = match extract_package_name_from_path(key) {
                Some(n) => n,
                None => continue,
            };
            // npm alias:路径是别名目录,条目内 "name" 字段才是 registry 真名
            let name = value["name"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or(path_name);
            let version = value["version"].as_str().unwrap_or_default().to_string();
            let tarball_url = value["resolved"].as_str().map(|s| s.to_string());
            // git/file 等非 registry 来源无法从 registry 缓存,跳过
            if let Some(url) = &tarball_url {
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    continue;
                }
            }

            let dep_key = format!("{}@{}", name, version);
            if !version.is_empty() && !seen.contains_key(&dep_key) {
                seen.insert(dep_key, true);
                deps.push(ParsedDependency {
                    name,
                    version,
                    tarball_url,
                });
            }
        }
    } else if let Some(deps_obj) = json["dependencies"].as_object() {
        // lockfileVersion 1（npm 6）：无 packages 字段，只有嵌套 dependencies 树
        collect_v1_dependencies(deps_obj, &mut deps, &mut seen);
    } else {
        return Err(
            "无法识别的 package-lock.json 格式（缺少 packages/dependencies 字段）".to_string(),
        );
    }

    Ok(deps)
}

/// 递归遍历 lockfileVersion 1 的嵌套 dependencies 树。
/// v1 alias 语义：version 形如 "npm:real-pkg@1.2.3"，真名/真版本在其中。
fn collect_v1_dependencies(
    deps_obj: &serde_json::Map<String, serde_json::Value>,
    out: &mut Vec<ParsedDependency>,
    seen: &mut HashMap<String, bool>,
) {
    for (key, value) in deps_obj {
        // bundled 依赖随父包 tarball 分发，无独立 registry 产物
        let bundled = value["bundled"].as_bool() == Some(true)
            || value["inBundle"].as_bool() == Some(true);

        if !bundled {
            if let Some(raw_version) = value["version"].as_str() {
                // alias："npm:real-pkg@1.2.3" → 名取 real-pkg，版本取 1.2.3
                let (name, version) = match raw_version.strip_prefix("npm:") {
                    Some(alias) => match alias.rfind('@') {
                        Some(pos) if pos > 0 => {
                            (alias[..pos].to_string(), alias[pos + 1..].to_string())
                        }
                        _ => (key.clone(), raw_version.to_string()),
                    },
                    None => (key.clone(), raw_version.to_string()),
                };

                let tarball_url = value["resolved"].as_str().map(|s| s.to_string());
                // git/file 等非 registry 来源跳过；v1 的 git 依赖 version 本身即 URL，一并过滤
                let non_registry = tarball_url
                    .as_ref()
                    .is_some_and(|u| !u.starts_with("http://") && !u.starts_with("https://"))
                    || version.contains(':')
                    || version.contains('/');

                if !non_registry && !version.is_empty() {
                    let dep_key = format!("{}@{}", name, version);
                    if !seen.contains_key(&dep_key) {
                        seen.insert(dep_key, true);
                        out.push(ParsedDependency {
                            name,
                            version,
                            tarball_url,
                        });
                    }
                }
            }
        }

        // 嵌套子依赖仍需递归（即使父级被过滤）
        if let Some(child) = value["dependencies"].as_object() {
            collect_v1_dependencies(child, out, seen);
        }
    }
}

/// 从 lockfile packages 路径提取包名:取最后一个 "node_modules/" 之后的部分
/// (嵌套条目如 "node_modules/foo/node_modules/bar" → "bar")。
/// 路径不含 node_modules/ 时返回 None(workspace 源目录条目)。
fn extract_package_name_from_path(path: &str) -> Option<String> {
    const MARKER: &str = "node_modules/";
    path.rfind(MARKER)
        .map(|pos| path[pos + MARKER.len()..].to_string())
        .filter(|n| !n.is_empty())
}

pub fn detect_and_parse(file_path: &Path) -> Result<Vec<ParsedDependency>, String> {
    let content = std::fs::read_to_string(file_path).map_err(|e| format!("读取文件失败: {}", e))?;

    let filename = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    match filename {
        "package.json" => parse_package_json(&content),
        "pnpm-lock.yaml" => parse_pnpm_lock(&content),
        "package-lock.json" => parse_package_lock(&content),
        _ => Err(format!("不支持的文件类型: {}", filename)),
    }
}

/// 解析单个依赖的 raw range 为具体版本。已是 x.y.z 直接短路；否则查共享
/// 版本缓存，未命中则请求 registry 并回填缓存。无法满足或请求失败时返回 None。
pub async fn resolve_single(
    http: &reqwest::Client,
    sem: &Arc<Semaphore>,
    cache: &Arc<Mutex<HashMap<String, Arc<Vec<String>>>>>,
    name: &str,
    raw_range: &str,
) -> Option<String> {
    if let Some(v) = pinned_version(raw_range) {
        return Some(v);
    }

    let versions = {
        let map = cache.lock().await;
        map.get(name).cloned()
    };
    let versions = match versions {
        Some(v) => v,
        None => {
            let fetched = fetch_versions(http, sem, name).await.ok()?;
            let arc = Arc::new(fetched);
            cache.lock().await.insert(name.to_string(), arc.clone());
            arc
        }
    };

    resolve_max_satisfying(raw_range, &versions)
}

/// Resolves raw semver ranges in dependency entries to concrete versions by
/// querying the npm registry. Already-pinned x.y.z versions short-circuit.
/// Entries whose range can't be satisfied are dropped silently.
pub async fn resolve_version_ranges(deps: Vec<ParsedDependency>) -> Vec<ParsedDependency> {
    let http = reqwest::Client::new();
    let sem = Arc::new(Semaphore::new(10));
    let versions_cache: Arc<Mutex<HashMap<String, Arc<Vec<String>>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let mut handles = Vec::with_capacity(deps.len());
    for dep in deps {
        let http = http.clone();
        let sem = sem.clone();
        let cache = versions_cache.clone();
        handles.push(tokio::spawn(async move {
            let resolved = resolve_single(&http, &sem, &cache, &dep.name, &dep.version).await?;
            Some(ParsedDependency {
                name: dep.name,
                version: resolved,
                tarball_url: dep.tarball_url,
            })
        }));
    }

    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(Some(dep)) = h.await {
            out.push(dep);
        }
    }
    out
}

pub async fn fetch_versions(
    http: &reqwest::Client,
    sem: &Arc<Semaphore>,
    name: &str,
) -> Result<Vec<String>, String> {
    let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
    let url = format!("https://registry.npmjs.org/{}", name);
    let resp = http
        .get(&url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let versions = body["versions"]
        .as_object()
        .map(|v| v.keys().cloned().collect())
        .unwrap_or_default();
    Ok(versions)
}

pub fn resolve_max_satisfying(range_str: &str, available: &[String]) -> Option<String> {
    let range = node_semver::Range::parse(range_str).ok()?;
    let mut matching: Vec<node_semver::Version> = available
        .iter()
        .filter_map(|v| node_semver::Version::parse(v).ok())
        .filter(|v| range.satisfies(v))
        .collect();
    matching.sort();
    matching.last().map(|v| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_package_json_dedups_optional_overriding_dependencies() {
        let content = r#"{
            "dependencies": { "shared": "^2.0.0", "only-dep": "1.0.0" },
            "optionalDependencies": { "shared": "^1.0.0" }
        }"#;
        let deps = parse_package_json(content).unwrap();
        // shared 仅一行，且采用 optionalDependencies 的范围
        let shared: Vec<_> = deps.iter().filter(|d| d.name == "shared").collect();
        assert_eq!(shared.len(), 1, "同名应去重为单行");
        assert_eq!(shared[0].version, "^1.0.0", "optional 应覆盖 dependencies");
        assert!(deps.iter().any(|d| d.name == "only-dep"));
    }

    #[test]
    fn parse_package_json_keeps_prod_range_over_dev() {
        // 同名同时在 dependencies / devDependencies：保留 prod range，单行
        let content = r#"{
            "dependencies": { "dup": "^2.0.0" },
            "devDependencies": { "dup": "^3.0.0" }
        }"#;
        let deps = parse_package_json(content).unwrap();
        let dup: Vec<_> = deps.iter().filter(|d| d.name == "dup").collect();
        assert_eq!(dup.len(), 1, "同名应去重为单行");
        assert_eq!(
            dup[0].version, "^2.0.0",
            "dependencies 应优先于 devDependencies"
        );
    }

    #[test]
    fn parse_package_json_invalid_optional_still_overrides_dependency() {
        let content = r#"{
            "dependencies": { "dup": "^2.0.0", "keep": "1.0.0" },
            "optionalDependencies": { "dup": "file:../dup" }
        }"#;
        let deps = parse_package_json(content).unwrap();

        assert!(
            deps.iter().all(|d| d.name != "dup"),
            "optionalDependencies 同名项即使不可解析，也应覆盖并移除 dependencies 旧条目"
        );
        assert!(deps.iter().any(|d| d.name == "keep"));
    }

    fn find<'a>(deps: &'a [ParsedDependency], name: &str) -> Vec<&'a ParsedDependency> {
        deps.iter().filter(|d| d.name == name).collect()
    }

    #[test]
    fn parse_package_lock_nested_node_modules_takes_last_segment() {
        // 嵌套版本冲突：顶层 bar@1.0.0 + foo 下嵌套 bar@2.0.0
        let content = r#"{
            "lockfileVersion": 3,
            "packages": {
                "": { "name": "root", "version": "0.0.0" },
                "node_modules/bar": { "version": "1.0.0", "resolved": "https://registry.npmjs.org/bar/-/bar-1.0.0.tgz" },
                "node_modules/foo": { "version": "3.0.0", "resolved": "https://registry.npmjs.org/foo/-/foo-3.0.0.tgz" },
                "node_modules/foo/node_modules/bar": { "version": "2.0.0", "resolved": "https://registry.npmjs.org/bar/-/bar-2.0.0.tgz" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        let bars = find(&deps, "bar");
        assert_eq!(bars.len(), 2, "嵌套条目应取最后一段包名，两个版本都保留");
        let versions: Vec<&str> = bars.iter().map(|d| d.version.as_str()).collect();
        assert!(versions.contains(&"1.0.0") && versions.contains(&"2.0.0"));
        assert!(
            deps.iter().all(|d| !d.name.contains("node_modules")),
            "不应出现含 node_modules 的坏名字"
        );
    }

    #[test]
    fn parse_package_lock_nested_scoped_package() {
        let content = r#"{
            "packages": {
                "node_modules/foo/node_modules/@scope/baz": { "version": "1.2.3" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "@scope/baz");
        assert_eq!(deps[0].version, "1.2.3");
    }

    #[test]
    fn parse_package_lock_skips_link_and_workspace_entries() {
        // workspace 源目录条目 + link 软链条目均应跳过
        let content = r#"{
            "packages": {
                "": { "name": "root", "version": "0.0.0" },
                "packages/app": { "name": "app", "version": "1.0.0" },
                "node_modules/app": { "link": true, "resolved": "packages/app" },
                "node_modules/real": { "version": "1.0.0" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "real");
    }

    #[test]
    fn parse_package_lock_alias_uses_name_field() {
        // npm alias：路径是别名，条目内 name 字段才是 registry 真名
        let content = r#"{
            "packages": {
                "node_modules/my-alias": { "name": "real-pkg", "version": "1.2.3" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "real-pkg");
        assert_eq!(deps[0].version, "1.2.3");
    }

    #[test]
    fn parse_package_lock_skips_git_and_file_resolved() {
        let content = r#"{
            "packages": {
                "node_modules/git-dep": { "version": "1.0.0", "resolved": "git+ssh://git@github.com/u/r.git#abc" },
                "node_modules/local-dep": { "version": "1.0.0", "resolved": "file:../local" },
                "node_modules/no-resolved": { "version": "2.0.0" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        assert_eq!(deps.len(), 1, "git/file 来源应被过滤");
        assert_eq!(deps[0].name, "no-resolved");
        assert!(deps[0].tarball_url.is_none());
    }

    #[test]
    fn parse_package_lock_root_entry_skipped() {
        let content = r#"{
            "packages": {
                "": { "name": "root", "version": "9.9.9" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        assert!(deps.is_empty(), "根条目不应产出依赖");
    }

    #[test]
    fn parse_package_lock_v1_recurses_nested_dependencies() {
        // v1：嵌套 dependencies 树，含同名不同版本
        let content = r#"{
            "lockfileVersion": 1,
            "dependencies": {
                "foo": {
                    "version": "3.0.0",
                    "resolved": "https://registry.npmjs.org/foo/-/foo-3.0.0.tgz",
                    "dependencies": {
                        "bar": { "version": "2.0.0", "resolved": "https://registry.npmjs.org/bar/-/bar-2.0.0.tgz" }
                    }
                },
                "bar": { "version": "1.0.0", "resolved": "https://registry.npmjs.org/bar/-/bar-1.0.0.tgz" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        assert_eq!(deps.len(), 3, "应递归收集全部嵌套依赖");
        let bars = find(&deps, "bar");
        assert_eq!(bars.len(), 2, "同名不同版本都保留");
    }

    #[test]
    fn parse_package_lock_v1_alias_version_field() {
        let content = r#"{
            "lockfileVersion": 1,
            "dependencies": {
                "my-alias": { "version": "npm:foo@2.0.0" },
                "scoped-alias": { "version": "npm:@scope/real@1.5.0" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        assert_eq!(deps.len(), 2);
        assert!(deps
            .iter()
            .any(|d| d.name == "foo" && d.version == "2.0.0"));
        assert!(deps
            .iter()
            .any(|d| d.name == "@scope/real" && d.version == "1.5.0"));
    }

    #[test]
    fn parse_package_lock_v1_skips_bundled_and_git() {
        let content = r#"{
            "lockfileVersion": 1,
            "dependencies": {
                "bundled-dep": { "version": "1.0.0", "bundled": true },
                "git-dep": { "version": "git+https://github.com/u/r.git#abc" },
                "git-resolved": { "version": "1.0.0", "resolved": "git+ssh://git@github.com/u/r.git#abc" },
                "normal": { "version": "1.0.0" }
            }
        }"#;
        let deps = parse_package_lock(content).unwrap();
        assert_eq!(deps.len(), 1, "bundled/git 条目应被过滤");
        assert_eq!(deps[0].name, "normal");
    }

    #[test]
    fn parse_package_lock_unrecognized_format_errors() {
        let content = r#"{ "lockfileVersion": 1 }"#;
        let err = parse_package_lock(content).unwrap_err();
        assert!(err.contains("无法识别"), "无依赖字段应报错: {}", err);
    }

    #[test]
    fn parse_pnpm_lock_v9_basic_and_peer_suffix() {
        let content = r#"
lockfileVersion: '9.0'
packages:
  foo@1.2.3:
    resolution: {integrity: sha512-xxx}
  '@scope/bar@2.0.0(react@18.2.0)':
    resolution: {integrity: sha512-yyy}
"#;
        let deps = parse_pnpm_lock(content).unwrap();
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().any(|d| d.name == "foo" && d.version == "1.2.3"));
        assert!(
            deps.iter()
                .any(|d| d.name == "@scope/bar" && d.version == "2.0.0"),
            "括号 peer 后缀应剥离: {:?}",
            deps
        );
    }

    #[test]
    fn parse_pnpm_lock_filters_git_and_file_entries() {
        let content = r#"
lockfileVersion: '9.0'
packages:
  pkg@https://codeload.github.com/u/r/tar.gz/abc123:
    resolution: {tarball: https://codeload.github.com/u/r/tar.gz/abc123}
  local@file:../local:
    resolution: {directory: ../local, type: directory}
  normal@1.0.0:
    resolution: {integrity: sha512-zzz}
"#;
        let deps = parse_pnpm_lock(content).unwrap();
        assert_eq!(deps.len(), 1, "git/file 条目应被过滤: {:?}", deps);
        assert_eq!(deps[0].name, "normal");
        assert_eq!(deps[0].version, "1.0.0");
    }

    #[test]
    fn parse_pnpm_lock_v6_leading_slash_keys() {
        let content = r#"
lockfileVersion: '6.0'
packages:
  /foo@1.2.3(peer@1.0.0):
    resolution: {integrity: sha512-xxx}
  /@scope/baz@0.5.0:
    resolution: {integrity: sha512-yyy}
"#;
        let deps = parse_pnpm_lock(content).unwrap();
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().any(|d| d.name == "foo" && d.version == "1.2.3"));
        assert!(deps
            .iter()
            .any(|d| d.name == "@scope/baz" && d.version == "0.5.0"));
    }

    #[test]
    fn parse_pnpm_lock_v5_peer_suffix_with_at() {
        // 回归：v5 peer 后缀含 '@' 时曾被 '@' 分支误切成
        // name="foo/1.2.3_react"、version="16.14.0"
        let content = r#"
lockfileVersion: 5.4
packages:
  /foo/1.2.3_react@16.14.0:
    resolution: {integrity: sha512-xxx}
  /@scope/bar/2.0.0_vue@3.0.0:
    resolution: {integrity: sha512-yyy}
"#;
        let deps = parse_pnpm_lock(content).unwrap();
        assert_eq!(deps.len(), 2, "{:?}", deps);
        assert!(
            deps.iter().any(|d| d.name == "foo" && d.version == "1.2.3"),
            "v5 peer 后缀应剥离: {:?}",
            deps
        );
        assert!(deps
            .iter()
            .any(|d| d.name == "@scope/bar" && d.version == "2.0.0"));
    }

    #[test]
    fn parse_pnpm_lock_v9_snapshots_fallback() {
        // 仅有 snapshots 时走 fallback，且同样过滤非 semver 条目
        let content = r#"
lockfileVersion: '9.0'
snapshots:
  foo@1.2.3: {}
  gitpkg@https://codeload.github.com/u/r/tar.gz/abc: {}
"#;
        let deps = parse_pnpm_lock(content).unwrap();
        assert_eq!(deps.len(), 1, "{:?}", deps);
        assert_eq!(deps[0].name, "foo");
        assert_eq!(deps[0].version, "1.2.3");
    }

    #[test]
    fn parse_pnpm_lock_dedups_same_name_version() {
        // 同包同版本不同 peer 组合 → 单条
        let content = r#"
lockfileVersion: '9.0'
packages:
  foo@1.2.3(react@17.0.0):
    resolution: {integrity: sha512-xxx}
  foo@1.2.3(react@18.2.0):
    resolution: {integrity: sha512-xxx}
"#;
        let deps = parse_pnpm_lock(content).unwrap();
        assert_eq!(deps.len(), 1, "同包同版本应去重: {:?}", deps);
    }
}
