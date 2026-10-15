use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use crate::version_picker::{metas_from_versions, pick_version, VersionMeta};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// 同时进行的 packument 请求数上限。
const MAX_CONCURRENT_FETCHES: usize = 16;
/// 网络错误 / 限流 / 网关错误时的最大尝试次数。
const MAX_FETCH_ATTEMPTS: u32 = 3;
/// npm 精简元数据（abbreviated packument）：只含安装所需字段，
/// 体积通常只有完整文档的几分之一，且已包含每个版本的依赖声明。
const ABBREVIATED_ACCEPT: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedDep {
    pub package_name: String,
    pub version: String,
}

/// 依赖解析进度，用于前端展示，判断是否卡住、卡在哪。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolveProgress {
    /// 已纳入结果集的包版本数
    pub resolved: usize,
    /// 已完成的元数据请求数（含失败）
    pub fetched: usize,
    /// 失败的元数据请求数
    pub failed: usize,
    /// 当前层仍在等待响应的请求数
    pub in_flight: usize,
    /// 当前 BFS 层级（从 1 开始）
    pub depth: usize,
    /// 最近完成的包名
    pub current: Option<String>,
    /// 部分仍在等待响应的包名（最多 3 个），用于定位慢请求
    pub waiting: Vec<String>,
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

/// BFS 待处理节点。`range` 为版本范围（根包为精确版本）；`optional_ctx`
/// 表示该节点是否位于某个 optional 子树内（一旦进入即保持 best-effort，
/// 子树解析失败时跳过而非中断整体）。
struct QueueItem {
    name: String,
    range: String,
    optional_ctx: bool,
}

/// 单个包名的元数据：一次请求同时得到版本列表、dist-tags 与各版本依赖。
struct Packument {
    metas: HashMap<String, VersionMeta>,
    dist_tags: HashMap<String, String>,
    manifests: serde_json::Map<String, serde_json::Value>,
}

impl Packument {
    fn from_json(mut body: serde_json::Value) -> Self {
        let manifests = match body.get_mut("versions").map(serde_json::Value::take) {
            Some(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        };
        let dist_tags = body["dist-tags"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            metas: metas_from_versions(&manifests),
            dist_tags,
            manifests,
        }
    }

    fn resolve(&self, range: &str, node: Option<&node_semver::Version>) -> Option<String> {
        pick_version(range, &self.metas, &self.dist_tags, node).or_else(|| {
            match node_semver::Range::parse(range) {
                // 无法解析的范围（git/url/alias 等）退回 latest
                Err(_) => self.dist_tags.get("latest").cloned(),
                Ok(_) => None,
            }
        })
    }
}

pub async fn resolve_all(
    packages: Vec<(String, String)>,
    registry: &str,
) -> Result<Vec<ResolvedDep>, String> {
    resolve_all_with_progress(packages, registry, None, |_| {}).await
}

/// 按 BFS 层级解析完整依赖闭包：每层先并发拉取本层所有未缓存包名的
/// packument，再在内存中完成版本匹配与下一层展开。`node` 为目标 Node 版本，
/// 用于按 npm 规则优先选择 engines 兼容的版本。
pub async fn resolve_all_with_progress<F>(
    packages: Vec<(String, String)>,
    registry: &str,
    node: Option<node_semver::Version>,
    mut on_progress: F,
) -> Result<Vec<ResolvedDep>, String>
where
    F: FnMut(&ResolveProgress),
{
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())?;
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES));
    let registry = registry.trim_end_matches('/').to_string();

    // required 语义强于 optional：同一包版本先在 optional 子树中访问，
    // 后续又作为 required 依赖出现时，会升级上下文并重新处理。
    let mut visited: HashMap<(String, String), VisitState> = HashMap::new();
    // 同一 (name, range) 只需入队一次，规则同 visited。
    let mut seen_specs: HashMap<(String, String), VisitState> = HashMap::new();
    let mut packuments: HashMap<String, Result<Arc<Packument>, String>> = HashMap::new();
    let mut result: Vec<ResolvedDep> = Vec::new();
    let mut progress = ResolveProgress {
        resolved: 0,
        fetched: 0,
        failed: 0,
        in_flight: 0,
        depth: 0,
        current: None,
        waiting: Vec::new(),
    };

    let mut frontier: Vec<QueueItem> = packages
        .into_iter()
        .map(|(name, version)| QueueItem {
            name,
            range: version,
            optional_ctx: false,
        })
        .collect();

    while !frontier.is_empty() {
        progress.depth += 1;

        let mut missing: HashSet<String> = frontier
            .iter()
            .filter(|item| !packuments.contains_key(&item.name))
            .map(|item| item.name.clone())
            .collect();
        if !missing.is_empty() {
            let mut set = JoinSet::new();
            for name in missing.iter().cloned() {
                let http = http.clone();
                let sem = semaphore.clone();
                let registry = registry.clone();
                set.spawn(async move {
                    let res = fetch_packument(&http, &sem, &registry, &name).await;
                    (name, res)
                });
            }
            progress.in_flight = missing.len();
            progress.waiting = missing.iter().take(3).cloned().collect();
            on_progress(&progress);

            while let Some(joined) = set.join_next().await {
                let (name, res) = joined.map_err(|e| e.to_string())?;
                missing.remove(&name);
                progress.fetched += 1;
                if res.is_err() {
                    progress.failed += 1;
                }
                progress.in_flight = missing.len();
                progress.waiting = missing.iter().take(3).cloned().collect();
                progress.current = Some(name.clone());
                packuments.insert(name, res.map(Arc::new));
                on_progress(&progress);
            }
        }

        let mut next: Vec<QueueItem> = Vec::new();
        for QueueItem {
            name,
            range,
            optional_ctx: opt_ctx,
        } in frontier.drain(..)
        {
            let packument = match packuments.get(&name) {
                Some(Ok(p)) => p.clone(),
                // 可选上下文内拉取失败时跳过；必需依赖仍报错，
                // 避免静默产出不完整缓存集
                Some(Err(e)) => {
                    if opt_ctx {
                        continue;
                    }
                    return Err(e.clone());
                }
                None => continue,
            };
            let Some(version) = packument.resolve(&range, node.as_ref()) else {
                continue;
            };

            let key = (name.clone(), version.clone());
            let next_state = VisitState::from_context(opt_ctx);
            match visited.get(&key).copied() {
                Some(VisitState::Required) => continue,
                Some(VisitState::Optional) if next_state == VisitState::Optional => continue,
                Some(VisitState::Optional) => {
                    visited.insert(key, VisitState::Required);
                }
                None => {
                    visited.insert(key, next_state);
                    result.push(ResolvedDep {
                        package_name: name.clone(),
                        version: version.clone(),
                    });
                }
            }

            let Some(manifest) = packument.manifests.get(&version) else {
                continue;
            };
            for DepSpec {
                name: dep_name,
                range: dep_range,
                optional: is_optional,
            } in extract_deps(manifest)
            {
                // 一旦进入 optional 子树即保持 best-effort
                let child_opt = opt_ctx || is_optional;
                let child_state = VisitState::from_context(child_opt);
                let spec_key = (dep_name, dep_range);
                match seen_specs.get(&spec_key).copied() {
                    Some(VisitState::Required) => continue,
                    Some(VisitState::Optional) if child_state == VisitState::Optional => continue,
                    _ => {
                        seen_specs.insert(spec_key.clone(), child_state);
                    }
                }
                next.push(QueueItem {
                    name: spec_key.0,
                    range: spec_key.1,
                    optional_ctx: child_opt,
                });
            }
        }

        progress.resolved = result.len();
        progress.in_flight = 0;
        progress.waiting.clear();
        on_progress(&progress);
        frontier = next;
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

async fn fetch_packument(
    http: &reqwest::Client,
    sem: &Arc<Semaphore>,
    registry: &str,
    name: &str,
) -> Result<Packument, String> {
    let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
    // scoped 包名中的 '/' 需编码，与 npm 客户端一致
    let url = format!("{}/{}", registry, name.replace('/', "%2f"));

    let mut attempt = 0;
    loop {
        attempt += 1;
        let retryable_err = match http
            .get(&url)
            .header("accept", ABBREVIATED_ACCEPT)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("解析 {} 元数据失败: {}", name, e))?;
                return Ok(Packument::from_json(body));
            }
            Ok(resp) => {
                let status = resp.status();
                let msg = format!("获取 {} 元数据失败 (HTTP {})", name, status);
                if !matches!(status.as_u16(), 429 | 502 | 503 | 504) {
                    return Err(msg);
                }
                msg
            }
            Err(e) => {
                let reason = if e.is_timeout() {
                    "请求超时".to_string()
                } else {
                    e.to_string()
                };
                format!("获取 {} 元数据失败: {}", name, reason)
            }
        };
        if attempt >= MAX_FETCH_ATTEMPTS {
            return Err(format!("{}（已重试 {} 次）", retryable_err, attempt - 1));
        }
        tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
    }
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

        /// 挂载一个只有 1.0.0 版本的精简 packument，`manifest` 为该版本的依赖声明。
        async fn mock_packument(server: &MockServer, name: &str, manifest: serde_json::Value) {
            Mock::given(method("GET"))
                .and(path(format!("/{}", name.replace('/', "%2f"))))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "name": name,
                    "dist-tags": { "latest": "1.0.0" },
                    "versions": { "1.0.0": manifest }
                })))
                .mount(server)
                .await;
        }

        async fn mock_failure(server: &MockServer, name: &str) {
            Mock::given(method("GET"))
                .and(path(format!("/{}", name)))
                .respond_with(ResponseTemplate::new(500))
                .mount(server)
                .await;
        }

        fn names(res: Vec<ResolvedDep>) -> Vec<String> {
            res.into_iter().map(|d| d.package_name).collect()
        }

        #[tokio::test]
        async fn optional_subtree_metadata_failure_does_not_abort() {
            let server = MockServer::start().await;
            // root a 依赖必需 b 与可选 opt
            mock_packument(
                &server,
                "a",
                json!({
                    "dependencies": { "b": "1.0.0" },
                    "optionalDependencies": { "opt": "1.0.0" }
                }),
            )
            .await;
            mock_packument(&server, "b", json!({})).await;
            // opt 的元数据返回 500 —— 属可选上下文，应跳过而非整体失败
            mock_failure(&server, "opt").await;

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri()).await;
            let names = names(res.expect("optional 子树失败不应中断整体解析"));
            assert!(names.contains(&"a".to_string()));
            assert!(names.contains(&"b".to_string()), "必需依赖应被解析");
        }

        #[tokio::test]
        async fn required_metadata_failure_still_errors() {
            let server = MockServer::start().await;
            mock_packument(&server, "a", json!({ "dependencies": { "b": "1.0.0" } })).await;
            // 必需依赖 b 的元数据失败 —— 必须报错，避免静默残缺缓存集
            mock_failure(&server, "b").await;

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri()).await;
            assert!(res.is_err(), "必需依赖元数据失败应返回错误");
        }

        #[tokio::test]
        async fn required_visit_after_optional_failure_still_errors() {
            let server = MockServer::start().await;
            mock_packument(
                &server,
                "opt-parent",
                json!({ "optionalDependencies": { "shared": "1.0.0" } }),
            )
            .await;
            mock_packument(
                &server,
                "req-parent",
                json!({ "dependencies": { "shared": "1.0.0" } }),
            )
            .await;
            // shared 从 optional 子树访问时应被跳过；同一 shared 又被 required
            // 子树触达时必须按必需上下文报错。
            mock_failure(&server, "shared").await;

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
            mock_packument(&server, "a", json!({ "peerDependencies": { "react": "1.0.0" } }))
                .await;
            mock_packument(&server, "react", json!({})).await;

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri()).await;
            let names = names(res.expect("peer 正常时应整体成功"));
            assert!(names.contains(&"react".to_string()), "peer 依赖应被解析并纳入");
        }

        #[tokio::test]
        async fn peer_dependency_failure_does_not_abort() {
            let server = MockServer::start().await;
            mock_packument(
                &server,
                "a",
                json!({
                    "dependencies": { "b": "1.0.0" },
                    "peerDependencies": { "p": "1.0.0" }
                }),
            )
            .await;
            mock_packument(&server, "b", json!({})).await;
            // peer p 的元数据 500 —— peer 按 optional 处理，跳过不中断
            mock_failure(&server, "p").await;

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri()).await;
            let names = names(res.expect("peer 子树失败不应中断整体解析"));
            assert!(names.contains(&"a".to_string()));
            assert!(names.contains(&"b".to_string()), "必需依赖应被解析");
        }

        #[tokio::test]
        async fn scoped_package_and_dist_tag_range_are_resolved() {
            let server = MockServer::start().await;
            mock_packument(
                &server,
                "@scope/root",
                json!({ "dependencies": { "@scope/child": "latest" } }),
            )
            .await;
            mock_packument(&server, "@scope/child", json!({})).await;

            let res = resolve_all(vec![("@scope/root".into(), "1.0.0".into())], &server.uri())
                .await
                .expect("scoped 包与 dist-tag 范围应可解析");
            let child = res
                .iter()
                .find(|d| d.package_name == "@scope/child")
                .expect("dist-tag 依赖应被纳入");
            assert_eq!(child.version, "1.0.0");
        }

        #[tokio::test]
        async fn shared_packument_is_fetched_once_and_progress_reported() {
            let server = MockServer::start().await;
            mock_packument(&server, "a", json!({ "dependencies": { "shared": "^1.0.0" } })).await;
            mock_packument(&server, "b", json!({ "dependencies": { "shared": "1.0.0" } })).await;
            Mock::given(method("GET"))
                .and(path("/shared"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "dist-tags": { "latest": "1.0.0" },
                    "versions": { "1.0.0": {} }
                })))
                .expect(1)
                .mount(&server)
                .await;

            let mut last: Option<ResolveProgress> = None;
            let res = resolve_all_with_progress(
                vec![("a".into(), "1.0.0".into()), ("b".into(), "1.0.0".into())],
                &server.uri(),
                None,
                |p| last = Some(p.clone()),
            )
            .await
            .expect("应解析成功");
            assert_eq!(res.len(), 3);
            let last = last.expect("应上报进度");
            assert_eq!(last.resolved, 3);
            assert_eq!(last.fetched, 3);
            assert_eq!(last.in_flight, 0);
        }

        #[tokio::test]
        async fn transitive_range_respects_target_node_engines() {
            let server = MockServer::start().await;
            mock_packument(&server, "a", json!({ "dependencies": { "dep": "^1.0.0" } })).await;
            Mock::given(method("GET"))
                .and(path("/dep"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "dist-tags": { "latest": "1.2.0" },
                    "versions": {
                        "1.1.0": { "engines": { "node": ">=16" } },
                        "1.2.0": { "engines": { "node": ">=20" } }
                    }
                })))
                .mount(&server)
                .await;

            let dep_version = |res: Vec<ResolvedDep>| {
                res.into_iter()
                    .find(|d| d.package_name == "dep")
                    .map(|d| d.version)
            };
            let node18 = node_semver::Version::parse("18.20.4").unwrap();
            let res = resolve_all_with_progress(
                vec![("a".into(), "1.0.0".into())],
                &server.uri(),
                Some(node18),
                |_| {},
            )
            .await
            .expect("应解析成功");
            assert_eq!(dep_version(res).as_deref(), Some("1.1.0"));

            let res = resolve_all(vec![("a".into(), "1.0.0".into())], &server.uri())
                .await
                .expect("应解析成功");
            assert_eq!(dep_version(res).as_deref(), Some("1.2.0"));
        }
    }
}
