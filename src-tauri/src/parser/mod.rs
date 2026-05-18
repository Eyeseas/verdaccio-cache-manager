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

    let mut deps = Vec::new();

    for field in ["dependencies", "devDependencies"] {
        if let Some(obj) = json[field].as_object() {
            for (name, version) in obj {
                if let Some(v) = version.as_str() {
                    if let Some(clean) = clean_semver_range(v) {
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
        "git+", "git:", "git@", "http://", "https://", "file:", "link:", "workspace:", "npm:",
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

    // Try '@' separator first (v6+/v9 format: "name@version" or "@scope/name@version")
    // For scoped packages, we need the LAST '@' that isn't at position 0
    if let Some(at_pos) = find_version_separator(key) {
        let name = &key[..at_pos];
        let version = &key[at_pos + 1..];
        // Strip any trailing parenthesized peer info like "(react@18.2.0)"
        let version = version.split('(').next().unwrap_or(version).trim();
        if !name.is_empty() && !version.is_empty() {
            return Some((name.to_string(), version.to_string()));
        }
    }

    // Try v5 format: "name/version" or "@scope/name/version"
    if let Some((name, version)) = parse_v5_key(key) {
        return Some((name, version));
    }

    None
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
    if key.starts_with('@') {
        // @scope/name/version - find the second '/'
        let after_scope = key.find('/')? + 1;
        let version_slash = key[after_scope..].find('/')?;
        let name = &key[..after_scope + version_slash];
        let version = &key[after_scope + version_slash + 1..];
        if !version.is_empty() && version.chars().next()?.is_ascii_digit() {
            return Some((name.to_string(), version.to_string()));
        }
    } else {
        // name/version - find the last '/' where what follows starts with a digit
        if let Some(slash_pos) = key.rfind('/') {
            let name = &key[..slash_pos];
            let version = &key[slash_pos + 1..];
            if !name.is_empty()
                && !version.is_empty()
                && version.chars().next()?.is_ascii_digit()
            {
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
            let name = extract_package_name_from_path(key);
            let version = value["version"].as_str().unwrap_or_default().to_string();
            let tarball_url = value["resolved"].as_str().map(|s| s.to_string());

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
    }

    Ok(deps)
}

fn extract_package_name_from_path(path: &str) -> String {
    let path = path.strip_prefix("node_modules/").unwrap_or(path);
    path.to_string()
}

pub fn detect_and_parse(file_path: &Path) -> Result<Vec<ParsedDependency>, String> {
    let content =
        std::fs::read_to_string(file_path).map_err(|e| format!("读取文件失败: {}", e))?;

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

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
