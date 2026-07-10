use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedDep {
    pub package_name: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Optional,
    Required,
}

impl VisitState {
    fn from_context(optional_context: bool) -> Self {
        if optional_context {
            Self::Optional
        } else {
            Self::Required
        }
    }
}

/// 从 version 元数据提取出的一条依赖声明。
struct DepSpec {
    name: String,
    /// 版本范围（semver range），非已解析的具体版本。
    range: String,
    /// 是否为可选依赖（声明于 optionalDependencies，或被同名 optional 覆盖）。
    optional: bool,
}

/// BFS 待处理节点。`optional_ctx` 表示该节点是否位于某个 optional 子树内
/// （一旦进入即保持 best-effort，子树解析失败时跳过而非中断整体）。
struct QueueItem {
    name: String,
    version: String,
    optional_ctx: bool,
}

pub async fn resolve_all(
    packages: Vec<(String, String)>,
    registry: &str,
) -> Result<Vec<ResolvedDep>, String> {
    let http = reqwest::Client::new();
    let semaphore = Arc::new(Semaphore::new(10));
    let mut visited: HashMap<(String, String), VisitState> = HashMap::new();
    let mut result: Vec<ResolvedDep> = Vec::new();
    // required 语义强于 optional：同一包版本先在 optional 子树中访问，
    // 后续又作为 required 依赖出现时，会升级上下文并重新处理。
    let mut queue: VecDeque<QueueItem> = VecDeque::new();

    // Cache: package name -> all available versions
    let mut versions_cache: HashMap<String, Vec<String>> = HashMap::new();

    for (name, version) in packages {
        queue.push_back(QueueItem {
            name,
            version,
            optional_ctx: false,
        });
    }

    while let Some(QueueItem {
        name,
        version,
        optional_ctx: opt_ctx,
    }) = queue.pop_front()
    {
        let key = (name.clone(), version.clone());
        let next_state = VisitState::from_context(opt_ctx);
        match visited.get(&key).copied() {
            Some(VisitState::Required) => continue,
            Some(VisitState::Optional) if next_state == VisitState::Optional => continue,
            Some(VisitState::Optional) => {
                // required 语义强于 optional；同一包版本后续以 required 身份触达时
                // 需要重新处理，以便 required 子依赖失败仍能正确报错。
                visited.insert(key.clone(), VisitState::Required);
            }
            None => {
                visited.insert(key.clone(), next_state);
                result.push(ResolvedDep {
                    package_name: name.clone(),
                    version: version.clone(),
                });
            }
        }

        let deps =
            match fetch_version_dependencies(&http, &semaphore, registry, &name, &version).await {
                Ok(d) => d,
                // optional 子树内的元数据拉取失败：跳过该子树，不中断整体
                Err(e) => {
                    if opt_ctx {
                        continue;
                    }
                    return Err(e);
                }
            };

        for DepSpec {
            name: dep_name,
            range: range_str,
            optional: is_optional,
        } in deps
        {
            // 一旦进入 optional 子树即保持 best-effort
            let child_opt = opt_ctx || is_optional;
            let all_versions = match versions_cache.get(&dep_name) {
                Some(v) => v.clone(),
                None => {
                    let v = match fetch_all_versions(&http, &semaphore, registry, &dep_name).await {
                        Ok(v) => v,
                        // 可选上下文内拉取失败时跳过；必需依赖仍报错，
                        // 避免静默产出不完整缓存集
                        Err(e) => {
                            if child_opt {
                                continue;
                            }
                            return Err(e);
                        }
                    };
                    versions_cache.insert(dep_name.clone(), v.clone());
                    v
                }
            };

            if let Some(resolved) = resolve_version(&range_str, &all_versions) {
                queue.push_back(QueueItem {
                    name: dep_name,
                    version: resolved,
                    optional_ctx: child_opt,
                });
            }
        }
    }

    Ok(result)
}

/// 从 version 元数据提取依赖。required 与 optional 合并；
/// 同名时按 npm 语义 optionalDependencies 覆盖 dependencies（采用 optional
/// 的版本范围，且安装失败可继续）。
/// peerDependencies 也纳入（npm 7+ 会自动安装 peer），按 optional 处理：
/// best-effort 解析，失败不阻断整体，且不覆盖同名的 regular/optional 声明。
fn extract_deps(body: &serde_json::Value) -> Vec<DepSpec> {
    let mut map: HashMap<String, (String, bool)> = HashMap::new();
    if let Some(obj) = body["dependencies"].as_object() {
        for (k, v) in obj {
            map.insert(k.clone(), (v.as_str().unwrap_or("*").to_string(), false));
        }
    }
    // optionalDependencies 后写入，按 npm 语义覆盖同名 dependencies
    if let Some(obj) = body["optionalDependencies"].as_object() {
        for (k, v) in obj {
            map.insert(k.clone(), (v.as_str().unwrap_or("*").to_string(), true));
        }
    }
    // peerDependencies 仅在同名不存在时插入，视作 optional
    if let Some(obj) = body["peerDependencies"].as_object() {
        for (k, v) in obj {
            map.entry(k.clone())
                .or_insert_with(|| (v.as_str().unwrap_or("*").to_string(), true));
        }
    }
    map.into_iter()
        .map(|(name, (range, optional))| DepSpec {
            name,
            range,
            optional,
        })
        .collect()
}

async fn fetch_version_dependencies(
    http: &reqwest::Client,
    sem: &Arc<Semaphore>,
    registry: &str,
    name: &str,
    version: &str,
) -> Result<Vec<DepSpec>, String> {
    let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
    let url = format!("{}/{}/{}", registry.trim_end_matches('/'), name, version);
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

    Ok(extract_deps(&body))
}

async fn fetch_all_versions(
    http: &reqwest::Client,
    sem: &Arc<Semaphore>,
    registry: &str,
    name: &str,
) -> Result<Vec<String>, String> {
    let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
    let url = format!("{}/{}", registry.trim_end_matches('/'), name);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn find<'a>(deps: &'a [DepSpec], name: &str) -> Option<&'a DepSpec> {
        deps.iter().find(|d| d.name == name)
    }

    #[test]
    fn extract_deps_merges_optional_and_required() {
        // 模拟 esbuild：dependencies 为空，平台二进制走 optionalDependencies
        let body = json!({
            "dependencies": {},
            "optionalDependencies": {
                "@esbuild/linux-x64": "0.21.5",
                "@esbuild/win32-x64": "0.21.5",
                "@esbuild/darwin-arm64": "0.21.5"
            }
        });
        let deps = extract_deps(&body);
        assert_eq!(deps.len(), 3);
        for plat in [
            "@esbuild/linux-x64",
            "@esbuild/win32-x64",
            "@esbuild/darwin-arm64",
        ] {
            let entry = find(&deps, plat).expect("平台包应被提取");
            assert_eq!(entry.range, "0.21.5");
            assert!(entry.optional, "应标记为可选依赖");
        }
    }

    #[test]
    fn extract_deps_optional_overrides_required() {
        // npm 语义：同名时 optionalDependencies 覆盖 dependencies
        let body = json!({
            "dependencies": { "shared": "^2.0.0" },
            "optionalDependencies": { "shared": "^1.0.0" }
        });
        let deps = extract_deps(&body);
        assert_eq!(deps.len(), 1);
        let entry = find(&deps, "shared").unwrap();
        assert_eq!(entry.range, "^1.0.0", "应采用 optional 的版本范围");
        assert!(entry.optional, "同名时应标记为可选（安装失败可继续）");
    }

    #[test]
    fn extract_deps_handles_missing_fields() {
        let body = json!({ "name": "no-deps", "version": "1.0.0" });
        assert!(extract_deps(&body).is_empty());
    }

    #[test]
    fn extract_deps_includes_peer_as_optional() {
        // npm 7+ 自动安装 peer，需纳入缓存集；按 optional 处理失败不阻断
        let body = json!({
            "dependencies": { "use-sync-external-store": "^1.0.0" },
            "peerDependencies": { "react": "^18.0.0" }
        });
        let deps = extract_deps(&body);
        let peer = find(&deps, "react").expect("peer 依赖应被提取");
        assert_eq!(peer.range, "^18.0.0");
        assert!(peer.optional, "peer 应标记为可选（best-effort）");
    }

    #[test]
    fn extract_deps_peer_does_not_override_regular_dep() {
        let body = json!({
            "dependencies": { "shared": "^2.0.0" },
            "peerDependencies": { "shared": ">=1" }
        });
        let deps = extract_deps(&body);
        assert_eq!(deps.len(), 1);
        let entry = find(&deps, "shared").unwrap();
        assert_eq!(entry.range, "^2.0.0", "同名时应保留 dependencies 的范围");
        assert!(!entry.optional, "同名时不应被 peer 降级为可选");
    }

    mod resolve_all_failure_tolerance {
        use super::*;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        async fn mock_version_list(server: &MockServer, name: &str) {
            Mock::given(method("GET"))
                .and(path(format!("/{}", name)))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "versions": { "1.0.0": {} } })),
                )
                .mount(server)
                .await;
        }

        async fn mock_meta(server: &MockServer, name: &str, body: serde_json::Value) {
            Mock::given(method("GET"))
                .and(path(format!("/{}/1.0.0", name)))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(server)
                .await;
        }

        #[tokio::test]
        async fn optional_subtree_metadata_failure_does_not_abort() {
            let server = MockServer::start().await;
            // root a 依赖必需 b 与可选 opt
            mock_meta(
                &server,
                "a",
                json!({
                    "dependencies": { "b": "1.0.0" },
                    "optionalDependencies": { "opt": "1.0.0" }
                }),
            )
            .await;
            mock_version_list(&server, "b").await;
            mock_version_list(&server, "opt").await;
            mock_meta(&server, "b", json!({})).await;
            // opt 的版本元数据返回 500 —— 属可选上下文，应跳过而非整体失败
            Mock::given(method("GET"))
                .and(path("/opt/1.0.0"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri()).await;
            let names: Vec<String> = res
                .expect("optional 子树失败不应中断整体解析")
                .into_iter()
                .map(|d| d.package_name)
                .collect();
            assert!(names.contains(&"a".to_string()));
            assert!(names.contains(&"b".to_string()), "必需依赖应被解析");
            assert!(names.contains(&"opt".to_string()), "opt 仍应进入缓存集");
        }

        #[tokio::test]
        async fn required_metadata_failure_still_errors() {
            let server = MockServer::start().await;
            mock_meta(&server, "a", json!({ "dependencies": { "b": "1.0.0" } })).await;
            mock_version_list(&server, "b").await;
            // 必需依赖 b 的元数据失败 —— 必须报错，避免静默残缺缓存集
            Mock::given(method("GET"))
                .and(path("/b/1.0.0"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri()).await;
            assert!(res.is_err(), "必需依赖元数据失败应返回错误");
        }

        #[tokio::test]
        async fn required_visit_after_optional_failure_still_errors() {
            let server = MockServer::start().await;
            mock_meta(
                &server,
                "opt-parent",
                json!({ "optionalDependencies": { "shared": "1.0.0" } }),
            )
            .await;
            mock_meta(
                &server,
                "req-parent",
                json!({ "dependencies": { "shared": "1.0.0" } }),
            )
            .await;
            mock_version_list(&server, "shared").await;

            // shared 先从 optional 子树被访问时应被跳过；随后 required 子树再访问
            // 同一 shared@1.0.0 时必须升级为必需上下文并报错。
            Mock::given(method("GET"))
                .and(path("/shared/1.0.0"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;

            let res = resolve_all(
                vec![
                    ("opt-parent".into(), "1.0.0".into()),
                    ("req-parent".into(), "1.0.0".into()),
                ],
                &server.uri(),
            )
            .await;
            assert!(
                res.is_err(),
                "同一包版本后续以 required 身份触达时不能被 optional visited 跳过"
            );
        }

        #[tokio::test]
        async fn peer_dependency_is_resolved_and_included() {
            let server = MockServer::start().await;
            // root a 声明 peer react —— npm 7+ 会自动安装，应进入缓存集
            mock_meta(
                &server,
                "a",
                json!({ "peerDependencies": { "react": "1.0.0" } }),
            )
            .await;
            mock_version_list(&server, "react").await;
            mock_meta(&server, "react", json!({})).await;

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri()).await;
            let names: Vec<String> = res
                .expect("peer 正常时应整体成功")
                .into_iter()
                .map(|d| d.package_name)
                .collect();
            assert!(names.contains(&"react".to_string()), "peer 依赖应被解析并纳入");
        }

        #[tokio::test]
        async fn peer_dependency_failure_does_not_abort() {
            let server = MockServer::start().await;
            mock_meta(
                &server,
                "a",
                json!({
                    "dependencies": { "b": "1.0.0" },
                    "peerDependencies": { "p": "1.0.0" }
                }),
            )
            .await;
            mock_version_list(&server, "b").await;
            mock_version_list(&server, "p").await;
            mock_meta(&server, "b", json!({})).await;
            // peer p 的版本元数据 500 —— peer 按 optional 处理，跳过不中断
            Mock::given(method("GET"))
                .and(path("/p/1.0.0"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri()).await;
            let names: Vec<String> = res
                .expect("peer 子树失败不应中断整体解析")
                .into_iter()
                .map(|d| d.package_name)
                .collect();
            assert!(names.contains(&"a".to_string()));
            assert!(names.contains(&"b".to_string()), "必需依赖应被解析");
        }
    }
}
