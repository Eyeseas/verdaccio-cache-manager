use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedDep {
    pub package_name: String,
    pub version: String,
}

pub async fn resolve_all(
    packages: Vec<(String, String)>,
) -> Result<Vec<ResolvedDep>, String> {
    let http = reqwest::Client::new();
    let semaphore = Arc::new(Semaphore::new(10));
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut result: Vec<ResolvedDep> = Vec::new();
    let mut queue: VecDeque<(String, String)> = VecDeque::new();

    // Cache: package name -> all available versions
    let mut versions_cache: HashMap<String, Vec<String>> = HashMap::new();

    for (name, version) in packages {
        queue.push_back((name, version));
    }

    while let Some((name, version)) = queue.pop_front() {
        let key = (name.clone(), version.clone());
        if visited.contains(&key) {
            continue;
        }
        visited.insert(key);
        result.push(ResolvedDep {
            package_name: name.clone(),
            version: version.clone(),
        });

        let deps = fetch_version_dependencies(&http, &semaphore, &name, &version).await?;

        for (dep_name, range_str) in deps {
            let all_versions = match versions_cache.get(&dep_name) {
                Some(v) => v.clone(),
                None => {
                    let v = fetch_all_versions(&http, &semaphore, &dep_name).await?;
                    versions_cache.insert(dep_name.clone(), v.clone());
                    v
                }
            };

            if let Some(resolved) = resolve_version(&range_str, &all_versions) {
                if !visited.contains(&(dep_name.clone(), resolved.clone())) {
                    queue.push_back((dep_name, resolved));
                }
            }
        }
    }

    Ok(result)
}

async fn fetch_version_dependencies(
    http: &reqwest::Client,
    sem: &Arc<Semaphore>,
    name: &str,
    version: &str,
) -> Result<HashMap<String, String>, String> {
    let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
    let url = format!("https://registry.npmjs.org/{}/{}", name, version);
    let resp = http
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("获取 {}@{} 元数据失败: {}", name, version, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "获取 {}@{} 元数据失败 (HTTP {})",
            name,
            version,
            resp.status()
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 {}@{} 元数据失败: {}", name, version, e))?;

    let deps = body["dependencies"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("*").to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(deps)
}

async fn fetch_all_versions(
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
        .map_err(|e| format!("获取 {} 版本列表失败: {}", name, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "获取 {} 版本列表失败 (HTTP {})",
            name,
            resp.status()
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 {} 版本列表失败: {}", name, e))?;

    let versions = body["versions"]
        .as_object()
        .map(|v| v.keys().cloned().collect())
        .unwrap_or_default();

    Ok(versions)
}

fn resolve_version(range_str: &str, available: &[String]) -> Option<String> {
    let range = match node_semver::Range::parse(range_str) {
        Ok(r) => r,
        Err(_) => return available.last().cloned(),
    };

    let mut matching: Vec<&String> = available
        .iter()
        .filter(|v| {
            node_semver::Version::parse(v)
                .map(|ver| range.satisfies(&ver))
                .unwrap_or(false)
        })
        .collect();

    matching.sort_by(|a, b| {
        let va = node_semver::Version::parse(a).unwrap();
        let vb = node_semver::Version::parse(b).unwrap();
        va.cmp(&vb)
    });

    matching.last().map(|v| (*v).clone())
}
