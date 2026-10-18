use crate::dependency_resolver::extract_deps;
use crate::parser::{pinned_version, ParsedDependency};
use crate::registry_client::RegistryClient;
use crate::version_picker::{metas_from_versions, pick_version};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    MissingTarball,
    MissingVersion,
    MissingPackage,
    CheckFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightItem {
    pub name: String,
    pub requested: String,
    pub version: Option<String>,
    pub status: Availability,
    pub message: Option<String>,
    pub latest_warning: Option<String>,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreflightReport {
    pub registry: String,
    pub items: Vec<PreflightItem>,
    pub plugin_used: bool,
}

#[derive(Clone, Serialize)]
pub struct PreflightProgress {
    pub request_id: String,
    pub completed: usize,
    pub total: usize,
    pub current: String,
}

#[derive(Clone)]
enum Metadata {
    Found(Arc<Value>),
    Missing,
    Failed(String),
}

async fn fetch_metadata(http: &reqwest::Client, registry: &str, name: &str) -> Metadata {
    let url = format!("{}/{}", registry, name.replace('/', "%2f"));
    for attempt in 0..3 {
        let error = match http
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await
        {
            Ok(response) if response.status().as_u16() == 404 => return Metadata::Missing,
            Ok(response) if response.status().is_success() => {
                return match response.json::<Value>().await {
                    Ok(body) if body["versions"].is_object() => Metadata::Found(Arc::new(body)),
                    Ok(_) => Metadata::Failed("元数据缺少 versions 对象".into()),
                    Err(e) => Metadata::Failed(format!("解析元数据失败: {}", e)),
                };
            }
            Ok(response) => {
                let status = response.status().as_u16();
                let error = format!("获取元数据失败 (HTTP {})", status);
                if !matches!(status, 429 | 500 | 502 | 503 | 504) {
                    return Metadata::Failed(error);
                }
                error
            }
            Err(e) => format!("获取元数据失败: {}", e),
        };
        if attempt == 2 {
            return Metadata::Failed(error);
        }
        tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
    }
    unreachable!()
}

async fn probe_tarball(http: &reqwest::Client, url: &str) -> Result<bool, String> {
    let response = http.head(url).send().await.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if matches!(status, 404 | 410 | 500) {
        return Ok(false);
    }
    // 部分仓库未实现 HEAD；GET 只读取响应头，随即丢弃响应体。
    if matches!(status, 405 | 501) {
        let response = http
            .get(url)
            .header("range", "bytes=0-0")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        return match response.status().as_u16() {
            200 | 206 => Ok(true),
            404 | 410 | 500 => Ok(false),
            status => Err(format!("探测 tarball 失败 (HTTP {})", status)),
        };
    }
    if response.status().is_success() {
        Ok(true)
    } else {
        Err(format!("探测 tarball 失败 (HTTP {})", status))
    }
}

/// lockfile 只检查锁定闭包；范围输入在目标仓库元数据上展开，绝不查公网兜底。
pub async fn check_install<F>(
    packages: Vec<ParsedDependency>,
    registry: &str,
    expand: bool,
    node: Option<node_semver::Version>,
    mut on_progress: F,
) -> Result<PreflightReport, String>
where
    F: FnMut(usize, usize, &str),
{
    let registry = registry.trim_end_matches('/');
    let parsed = reqwest::Url::parse(registry).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Registry 必须使用 http 或 https".into());
    }
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let plugin = tokio::time::timeout(
        Duration::from_secs(15),
        RegistryClient::new(registry).fetch_plugin_entries(true),
    )
    .await;
    let mut cached: HashMap<String, HashSet<String>> = HashMap::new();
    if let Ok(Ok(entries)) = plugin {
        for entry in entries {
            if let (Some(name), Some(versions)) =
                (entry["name"].as_str(), entry["cached_versions"].as_array())
            {
                cached.insert(
                    name.into(),
                    versions
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect(),
                );
            }
        }
    }
    let plugin_used = !cached.is_empty();
    let semaphore = Arc::new(Semaphore::new(16));
    let mut metadata = HashMap::new();
    let mut seen: HashMap<(String, String), bool> = HashMap::new();
    let mut frontier: Vec<_> = packages
        .into_iter()
        .map(|p| (p.name, p.version, false))
        .collect();
    let mut items: Vec<PreflightItem> = Vec::new();
    let mut probes: HashMap<(String, String), (Availability, Option<String>)> = HashMap::new();
    while !frontier.is_empty() {
        let mut current = Vec::new();
        for (name, range, optional) in frontier.drain(..) {
            let key = (name.clone(), range.clone());
            if let Some(previous_optional) = seen.get(&key) {
                if !previous_optional || optional {
                    continue;
                }
                // 必需路径覆盖之前的可选路径，并继续升级其子依赖。
                items.retain(|i| i.name != name || i.requested != range);
            }
            seen.insert(key, optional);
            current.push((name, range, optional));
        }
        let total = items.len() + current.len();
        on_progress(items.len(), total, "读取内网元数据");
        let names: HashSet<_> = current
            .iter()
            .map(|(n, _, _)| n.clone())
            .filter(|n| !metadata.contains_key(n))
            .collect();
        let mut fetches = JoinSet::new();
        for name in names {
            let http = http.clone();
            let registry = registry.to_string();
            let sem = semaphore.clone();
            fetches.spawn(async move {
                let _permit = sem.acquire().await;
                let result = fetch_metadata(&http, &registry, &name).await;
                (name, result)
            });
        }
        while let Some(result) = fetches.join_next().await {
            let (name, result) = result.map_err(|e| e.to_string())?;
            on_progress(items.len(), total, &name);
            metadata.insert(name, result);
        }
        let mut checks = JoinSet::new();
        for (name, requested, optional) in current {
            let mut item = PreflightItem {
                name: name.clone(),
                requested: requested.clone(),
                version: pinned_version(&requested),
                status: Availability::CheckFailed,
                message: None,
                latest_warning: None,
                optional,
            };
            let body = match &metadata[&name] {
                Metadata::Missing => {
                    item.status = Availability::MissingPackage;
                    items.push(item);
                    continue;
                }
                Metadata::Failed(error) => {
                    item.message = Some(error.clone());
                    items.push(item);
                    continue;
                }
                Metadata::Found(body) => body.clone(),
            };
            let versions = body["versions"].as_object().expect("validated versions");
            let tags: HashMap<String, String> = body["dist-tags"]
                .as_object()
                .map(|tags| {
                    tags.iter()
                        .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.into())))
                        .collect()
                })
                .unwrap_or_default();
            item.version = pick_version(
                &requested,
                &metas_from_versions(versions),
                &tags,
                node.as_ref(),
            )
            .or(item.version);
            let Some(version) = item
                .version
                .as_ref()
                .filter(|v| versions.contains_key(*v))
                .cloned()
            else {
                item.status = Availability::MissingVersion;
                item.message = Some("目标仓库没有满足要求的版本".into());
                items.push(item);
                continue;
            };
            if expand {
                for dep in extract_deps(&versions[&version]) {
                    frontier.push((dep.name, dep.range, optional || dep.optional));
                }
            }
            let key = (name.clone(), version.clone());
            let known = probes.get(&key).cloned().or_else(|| {
                cached.get(&name).map(|v| {
                    (
                        if v.contains(&version) {
                            Availability::Available
                        } else {
                            Availability::MissingTarball
                        },
                        None,
                    )
                })
            });
            let http = http.clone();
            let sem = semaphore.clone();
            let url = versions[&version]["dist"]["tarball"]
                .as_str()
                .map(str::to_string);
            let latest_selected =
                tags.get("latest") == Some(&version) && pinned_version(&requested).is_none();
            checks.spawn(async move {
                let _permit = sem.acquire().await;
                let (status, message) = match known {
                    Some(result) => result,
                    None => match url {
                        None => (
                            Availability::MissingTarball,
                            Some("版本没有 dist.tarball".into()),
                        ),
                        Some(url) => match probe_tarball(&http, &url).await {
                            Ok(true) => (Availability::Available, None),
                            Ok(false) => (
                                Availability::MissingTarball,
                                Some("tarball 不可下载（404/410/500）".into()),
                            ),
                            Err(error) => (Availability::CheckFailed, Some(error)),
                        },
                    },
                };
                item.status = status;
                item.message = message;
                if latest_selected && item.status == Availability::MissingTarball {
                    item.latest_warning = Some(format!(
                        "npm 优先选择 latest={}，但该版本无 tgz；其他版本可用也不会自动回退",
                        version
                    ));
                }
                (key, item)
            });
        }
        while let Some(result) = checks.join_next().await {
            let (key, item) = result.map_err(|e| e.to_string())?;
            probes.insert(key, (item.status.clone(), item.message.clone()));
            let name = item.name.clone();
            items.push(item);
            on_progress(items.len(), total + frontier.len(), &name);
        }
    }
    items.sort_by(|a, b| (&a.name, &a.requested).cmp(&(&b.name, &b.requested)));
    on_progress(items.len(), items.len(), "完成");
    Ok(PreflightReport {
        registry: registry.into(),
        items,
        plugin_used,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn dep(name: &str, version: &str) -> ParsedDependency {
        ParsedDependency {
            name: name.into(),
            version: version.into(),
            tarball_url: None,
        }
    }

    #[tokio::test]
    async fn plugin_checks_selected_latest_and_expands_inner_registry_dependencies() {
        let server = MockServer::start().await;
        Mock::given(path("/-/cached-packages")).respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"name":"root","cached_versions":["1.9.0"]}, {"name":"child","cached_versions":["2.0.0"]}
        ]))).mount(&server).await;
        Mock::given(path("/root")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "dist-tags":{"latest":"1.0.0"},"versions":{"1.0.0":{"dependencies":{"child":"^2"}},"1.9.0":{}}
        }))).mount(&server).await;
        Mock::given(path("/child"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "versions":{"2.0.0":{}}
            })))
            .mount(&server)
            .await;
        let report = check_install(
            vec![dep("root", "^1")],
            &server.uri(),
            true,
            None,
            |_, _, _| {},
        )
        .await
        .unwrap();
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].status, Availability::Available);
        assert_eq!(report.items[1].version.as_deref(), Some("1.0.0"));
        assert!(report.items[1].latest_warning.is_some());
        assert_eq!(report.items[1].status, Availability::MissingTarball);
    }

    #[tokio::test]
    async fn distinguishes_missing_package_version_tarball_and_permissions() {
        let server = MockServer::start().await;
        for (name, status, body) in [
            ("absent", 404, json!({})),
            ("denied", 403, json!({})),
            ("old", 200, json!({"versions":{"1.0.0":{}}})),
            (
                "ghost",
                200,
                json!({"versions":{"2.0.0":{"dist":{"tarball":format!("{}/ghost.tgz",server.uri())}}}}),
            ),
        ] {
            Mock::given(path(format!("/{}", name)))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .mount(&server)
                .await;
        }
        Mock::given(method("HEAD"))
            .and(path("/ghost.tgz"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let report = check_install(
            ["absent", "denied", "old", "ghost"]
                .map(|n| dep(n, "2.0.0"))
                .to_vec(),
            &server.uri(),
            false,
            None,
            |_, _, _| {},
        )
        .await
        .unwrap();
        assert_eq!(
            report.items.iter().map(|i| &i.status).collect::<Vec<_>>(),
            vec![
                &Availability::MissingPackage,
                &Availability::CheckFailed,
                &Availability::MissingTarball,
                &Availability::MissingVersion
            ]
        );
    }

    #[tokio::test]
    async fn head_falls_back_to_range_get_and_lockfile_does_not_expand() {
        let server = MockServer::start().await;
        Mock::given(path("/root")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":{"1.0.0":{
            "dependencies":{"absent":"^1"}, "dist":{"tarball":format!("{}/root.tgz",server.uri())}
        }}}))).mount(&server).await;
        Mock::given(method("HEAD"))
            .and(path("/root.tgz"))
            .respond_with(ResponseTemplate::new(405))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/root.tgz"))
            .and(header("range", "bytes=0-0"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(vec![1]))
            .expect(1)
            .mount(&server)
            .await;
        let report = check_install(
            vec![dep("root", "1.0.0")],
            &server.uri(),
            false,
            None,
            |_, _, _| {},
        )
        .await
        .unwrap();
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].status, Availability::Available);
    }
}
