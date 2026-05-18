use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub tarball_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub name: String,
    pub description: Option<String>,
    pub latest_version: Option<String>,
    pub versions: Vec<String>,
    #[serde(default)]
    pub cached_versions: Vec<String>,
}

pub struct RegistryClient {
    http: reqwest::Client,
    pub registry_url: String,
    token: OnceCell<String>,
}

/// npm tarball 文件名：scoped 包只取最后一段（`@scope/name` → `name-x.y.z.tgz`），
/// 对齐 npm registry 的 `/{name}/-/{filename}` 路径约定。
pub fn tarball_filename(name: &str, version: &str) -> String {
    let name_part = name.rsplit('/').next().unwrap_or(name);
    format!("{}-{}.tgz", name_part, version)
}

/// 完整 tarball URL：`{registry}/{name}/-/{filename}`。
pub fn tarball_url(registry_url: &str, name: &str, version: &str) -> String {
    format!(
        "{}/{}/-/{}",
        registry_url.trim_end_matches('/'),
        name,
        tarball_filename(name, version)
    )
}

impl RegistryClient {
    pub fn new(registry_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            registry_url: registry_url.to_string(),
            token: OnceCell::new(),
        }
    }

    pub async fn ensure_token(&self) -> Result<(), String> {
        self.token
            .get_or_try_init(|| async {
                let url = format!(
                    "{}/-/user/org.couchdb.user:cache-manager",
                    self.registry_url
                );
                let body = serde_json::json!({
                    "name": "cache-manager",
                    "password": "cache-manager-auto"
                });

                let resp = self
                    .http
                    .put(&url)
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| format!("登录失败: {}", e))?;

                let json: serde_json::Value =
                    resp.json().await.map_err(|e| e.to_string())?;

                let token = if let Some(t) = json["token"].as_str() {
                    t.to_string()
                } else if let Some(t) = json["ok"].as_str() {
                    t.to_string()
                } else {
                    use base64::Engine;
                    let basic = base64::engine::general_purpose::STANDARD
                        .encode("cache-manager:cache-manager-auto");
                    format!("Basic {}", basic)
                };

                Ok::<String, String>(token)
            })
            .await?;
        Ok(())
    }

    pub async fn test_connection(&self) -> Result<(), String> {
        let url = format!("{}/-/ping", self.registry_url);
        self.http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("连接失败: {}", e))?;
        Ok(())
    }

    pub async fn list_cached_via_plugin(&self) -> Result<Vec<SearchResult>, String> {
        let url = format!("{}/-/cached-packages", self.registry_url);
        let resp = self
            .http
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("请求插件接口失败: {}", e))?;

        let status = resp.status();
        if status.as_u16() == 404 {
            return Err("PLUGIN_NOT_INSTALLED".to_string());
        }
        if !status.is_success() {
            return Err(format!("插件接口请求失败 (HTTP {})", status));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("解析响应失败: {}", e))?;

        let arr = body
            .as_array()
            .ok_or_else(|| "响应不是数组".to_string())?;

        let results = arr
            .iter()
            .filter_map(|pkg| {
                let name = pkg["name"].as_str()?.to_string();
                let str_arr = |key: &str| -> Vec<String> {
                    pkg[key]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let versions = str_arr("versions");
                let cached_versions = str_arr("cached_versions");
                Some(SearchResult {
                    name,
                    description: pkg["description"].as_str().map(|s| s.to_string()),
                    latest_version: pkg["latest"]
                        .as_str()
                        .map(|s| s.to_string())
                        .or_else(|| cached_versions.last().cloned())
                        .or_else(|| versions.last().cloned()),
                    versions,
                    cached_versions,
                })
            })
            .collect();

        Ok(results)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        let url = format!("{}/-/v1/search?text={}&size=20", self.registry_url, query);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        let results = body["objects"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|obj| {
                let pkg = &obj["package"];
                Some(SearchResult {
                    name: pkg["name"].as_str()?.to_string(),
                    description: pkg["description"].as_str().map(|s| s.to_string()),
                    latest_version: pkg["version"].as_str().map(|s| s.to_string()),
                    versions: vec![],
                    cached_versions: vec![],
                })
            })
            .collect();

        Ok(results)
    }

    pub async fn get_package_versions(&self, name: &str) -> Result<Vec<String>, String> {
        let url = format!("{}/{}", self.registry_url, name);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        let versions = body["versions"]
            .as_object()
            .map(|v| v.keys().cloned().collect())
            .unwrap_or_default();

        Ok(versions)
    }

    pub async fn download_tarball(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    }

    pub async fn trigger_proxy_cache(&self, package_name: &str, version: &str) -> Result<(), String> {
        let url = tarball_url(&self.registry_url, package_name, version);

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("代理缓存请求失败: {}", e))?;

        if resp.status().is_success() {
            let _ = resp.bytes().await;
            Ok(())
        } else if resp.status().as_u16() == 404 {
            Err(format!("包 {}@{} 在上游不存在", package_name, version))
        } else {
            Err(format!("代理缓存失败 ({})", resp.status()))
        }
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = self.token.get() {
            if token.starts_with("Basic ") {
                return req.header("authorization", token.as_str());
            }
            return req.header("authorization", format!("Bearer {}", token));
        }
        req
    }

    async fn fetch_packument(&self, name: &str) -> Result<serde_json::Value, String> {
        let url = format!("{}/{}", self.registry_url, name);
        let resp = self
            .apply_auth(self.http.get(&url))
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("获取包元数据失败: {}", e))?;

        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(format!("包 {} 不存在", name));
        }
        if !status.is_success() {
            return Err(format!("获取包元数据失败 (HTTP {})", status));
        }
        resp.json()
            .await
            .map_err(|e| format!("解析包元数据失败: {}", e))
    }

    /// 从一组版本号中按 semver 选出最大版本，解析失败时回退到字符串比较。
    fn max_version<'a, I: Iterator<Item = &'a String>>(
        versions: I,
    ) -> Option<String> {
        let mut best: Option<(Option<node_semver::Version>, String)> = None;
        for v in versions {
            let parsed = node_semver::Version::parse(v).ok();
            let take = match (&best, &parsed) {
                (None, _) => true,
                (Some((Some(b), _)), Some(p)) => p > b,
                (Some((Some(_), _)), None) => false,
                (Some((None, b)), _) => v.as_str() > b.as_str(),
            };
            if take {
                best = Some((parsed, v.clone()));
            }
        }
        best.map(|(_, s)| s)
    }

    /// 整包删除：DELETE /{name}/-rev/{rev}，404 视为已删除（幂等）。
    async fn delete_whole_package(
        &self,
        name: &str,
        rev: &str,
    ) -> Result<(), String> {
        let url = format!("{}/{}/-rev/{}", self.registry_url, name, rev);
        let resp = self
            .apply_auth(self.http.delete(&url))
            .send()
            .await
            .map_err(|e| format!("删除包失败: {}", e))?;
        if resp.status().as_u16() == 404 {
            return Ok(());
        }
        self.check_manage_response(resp, name, None).await
    }

    /// 删除磁盘上的 tarball。404 视为已删除（幂等）。
    async fn delete_tarball(
        &self,
        name: &str,
        version: &str,
        rev: &str,
    ) -> Result<(), String> {
        let filename = tarball_filename(name, version);
        let url = format!(
            "{}/{}/-/{}/-rev/{}",
            self.registry_url, name, filename, rev
        );
        let resp = self
            .apply_auth(self.http.delete(&url))
            .send()
            .await
            .map_err(|e| format!("删除 tarball 失败: {}", e))?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 404 {
            return Ok(());
        }
        match status.as_u16() {
            401 | 403 => Err(format!("权限不足，无法删除 {}@{}", name, version)),
            _ => {
                let text = resp.text().await.unwrap_or_default();
                Err(format!("删除 tarball 失败 ({}): {}", status, text))
            }
        }
    }

    /// 删除指定版本。version 为 None 时删除整个包。
    ///
    /// 单版本流程必须同时更新 packument 元数据并物理删除 tarball：
    /// Verdaccio 的缓存索引由存储目录中的 .tgz 文件决定，若只改元数据
    /// 而保留 tarball，下次扫描会把版本重新索引回来。操作设计为幂等，
    /// 元数据中已无该版本时不报错，继续清理残留的 tarball。
    pub async fn unpublish_package(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<(), String> {
        self.ensure_token().await?;

        let doc = match self.fetch_packument(name).await {
            Ok(d) => d,
            // 包已不存在：视为已删除，交由上层清理本地索引
            Err(e) if e.contains("不存在") => return Ok(()),
            Err(e) => return Err(e),
        };
        let rev = doc["_rev"]
            .as_str()
            .unwrap_or("0-0000000000000000")
            .to_string();

        let ver = match version {
            None => return self.delete_whole_package(name, &rev).await,
            Some(v) => v,
        };

        let in_meta = doc["versions"]
            .get(ver)
            .map(|v| !v.is_null())
            .unwrap_or(false);

        // 删除最后一个版本等价于整包 unpublish（对齐 npm CLI 行为）
        let remaining: Vec<String> = doc["versions"]
            .as_object()
            .map(|o| o.keys().filter(|k| k.as_str() != ver).cloned().collect())
            .unwrap_or_default();
        if in_meta && remaining.is_empty() {
            return self.delete_whole_package(name, &rev).await;
        }

        // 当前用于删 tarball 的 rev；PUT 成功后会刷新为最新值
        let mut tarball_rev = rev.clone();

        if in_meta {
            let mut doc = doc;
            if let Some(obj) = doc["versions"].as_object_mut() {
                obj.remove(ver);
            }
            if let Some(obj) = doc["time"].as_object_mut() {
                obj.remove(ver);
            }
            // CouchDB 风格的内部字段不应随 unpublish 写回（对齐 libnpmpublish）
            if let Some(map) = doc.as_object_mut() {
                map.remove("_attachments");
                map.remove("_revisions");
            }
            // 仅当被删版本是某个 dist-tag 时移除该 tag；
            // 若移除的是 latest，则把 latest 指向剩余最高版本。
            let removed_latest = doc["dist-tags"]["latest"].as_str()
                == Some(ver);
            if let Some(tags) = doc["dist-tags"].as_object_mut() {
                let stale: Vec<String> = tags
                    .iter()
                    .filter(|(_, v)| v.as_str() == Some(ver))
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in stale {
                    tags.remove(&k);
                }
            }
            if removed_latest {
                if let Some(latest) =
                    Self::max_version(remaining.iter())
                {
                    doc["dist-tags"]["latest"] =
                        serde_json::Value::String(latest);
                }
            }

            let url =
                format!("{}/{}/-rev/{}", self.registry_url, name, rev);
            let resp = self
                .apply_auth(self.http.put(&url))
                .header("content-type", "application/json")
                .json(&doc)
                .send()
                .await
                .map_err(|e| format!("提交修改失败: {}", e))?;
            // 404 表示元数据侧已无该版本，继续删 tarball
            if resp.status().as_u16() != 404 {
                self.check_manage_response(resp, name, Some(ver)).await?;
            }

            // PUT 成功后 revision 已变化，重新拉取以拿到最新 _rev，
            // 否则 tarball DELETE 易因 rev 过期失败或留下半完成状态。
            match self.fetch_packument(name).await {
                Ok(fresh) => {
                    if let Some(r) = fresh["_rev"].as_str() {
                        tarball_rev = r.to_string();
                    }
                }
                // 包整体已不存在：tarball 也已随之删除
                Err(e) if e.contains("不存在") => return Ok(()),
                Err(_) => {}
            }
        }

        // 物理删除 tarball，避免存储扫描重新索引
        self.delete_tarball(name, ver, &tarball_rev).await
    }

    /// 标记指定版本为废弃。
    pub async fn deprecate_package(
        &self,
        name: &str,
        version: &str,
        message: &str,
    ) -> Result<(), String> {
        self.ensure_token().await?;
        let mut doc = self.fetch_packument(name).await?;

        let target = doc
            .get_mut("versions")
            .and_then(|v| v.get_mut(version))
            .filter(|v| !v.is_null())
            .ok_or_else(|| format!("版本 {}@{} 不存在", name, version))?;
        target["deprecated"] = serde_json::Value::String(message.to_string());

        let url = format!("{}/{}", self.registry_url, name);
        let resp = self
            .apply_auth(self.http.put(&url))
            .header("content-type", "application/json")
            .json(&doc)
            .send()
            .await
            .map_err(|e| format!("提交修改失败: {}", e))?;
        self.check_manage_response(resp, name, Some(version)).await
    }

    async fn check_manage_response(
        &self,
        resp: reqwest::Response,
        name: &str,
        version: Option<&str>,
    ) -> Result<(), String> {
        if resp.status().is_success() {
            return Ok(());
        }
        let status = resp.status();
        let target = match version {
            Some(v) => format!("{}@{}", name, v),
            None => name.to_string(),
        };
        match status.as_u16() {
            401 | 403 => Err(format!("权限不足，无法操作 {}", target)),
            404 => Err(format!("{} 不存在", target)),
            409 => Err(format!("操作冲突，请刷新后重试 ({})", target)),
            _ => {
                let text = resp.text().await.unwrap_or_default();
                Err(format!("操作失败 ({}): {}", status, text))
            }
        }
    }

    pub async fn publish_package(
        &self,
        name: &str,
        version: &str,
        metadata: serde_json::Value,
        tarball: &[u8],
    ) -> Result<(), String> {
        self.ensure_token().await?;

        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(tarball);
        let tarball_name = format!("{}-{}.tgz", name, version);

        let body = serde_json::json!({
            "name": name,
            "versions": {
                version: metadata
            },
            "_attachments": {
                tarball_name: {
                    "content_type": "application/octet-stream",
                    "data": encoded
                }
            }
        });

        let url = format!("{}/{}", self.registry_url, name);
        let mut req = self
            .http
            .put(&url)
            .header("content-type", "application/json");

        if let Some(token) = self.token.get() {
            if token.starts_with("Basic ") {
                req = req.header("authorization", token.as_str());
            } else {
                req = req.header("authorization", format!("Bearer {}", token));
            }
        }

        let resp = req.json(&body).send().await.map_err(|e| e.to_string())?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            if status.as_u16() == 413 {
                return Err(format!(
                    "包体积超出 Verdaccio 限制（413 Payload Too Large）。请在服务端 config.yaml 中将 max_body_size 调大（建议 100mb）后重启 Verdaccio。当前包：{}@{}",
                    name, version
                ));
            }
            let text = resp.text().await.unwrap_or_default();
            Err(format!("Publish 失败 ({}): {}", status, text))
        }
    }
}

/// 从 npmjs.org 批量下载 tarball 到目录。
///
/// 磁盘文件名保留 scope 前缀（`@scope/name` → `@scope-name-x.y.z.tgz`），
/// 与下载用的 registry URL 路径段（仅末段）规则不同，不能混用。
pub async fn download_tarballs_to_dir<F>(
    packages: &[(String, String)],
    output_dir: &std::path::Path,
    on_progress: F,
) -> Result<usize, String>
where
    F: Fn(usize, usize, &str),
{
    use tokio::fs;

    fs::create_dir_all(output_dir)
        .await
        .map_err(|e| format!("创建目录失败: {}", e))?;

    let http = reqwest::Client::new();
    let total = packages.len();
    let mut completed = 0usize;

    for (name, version) in packages {
        let disk_name = if name.starts_with('@') {
            name.replace('/', "-")
        } else {
            name.clone()
        };
        let file_path = output_dir.join(format!("{}-{}.tgz", disk_name, version));

        on_progress(completed, total, &format!("{}@{}", name, version));

        let url = tarball_url("https://registry.npmjs.org", name, version);

        match http.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
                fs::write(&file_path, &bytes)
                    .await
                    .map_err(|e| format!("写入文件失败: {}", e))?;
                completed += 1;
            }
            Ok(resp) => {
                return Err(format!(
                    "下载 {}@{} 失败 (HTTP {})",
                    name, version, resp.status()
                ));
            }
            Err(e) => {
                return Err(format!("下载 {}@{} 失败: {}", name, version, e));
            }
        }
    }

    on_progress(completed, total, "");
    Ok(completed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn tarball_url_handles_scoped_and_plain_packages() {
        assert_eq!(
            tarball_url("https://r.npmjs.org", "react", "18.0.0"),
            "https://r.npmjs.org/react/-/react-18.0.0.tgz"
        );
        assert_eq!(
            tarball_url("https://r.npmjs.org", "@babel/core", "7.1.0"),
            "https://r.npmjs.org/@babel/core/-/core-7.1.0.tgz"
        );
    }

    #[test]
    fn tarball_url_trims_trailing_slash() {
        assert_eq!(
            tarball_url("http://localhost:4873/", "lodash", "4.17.21"),
            "http://localhost:4873/lodash/-/lodash-4.17.21.tgz"
        );
    }

    #[test]
    fn max_version_picks_highest_semver() {
        let vs = vec![s("1.0.0"), s("2.10.0"), s("2.9.1"), s("0.1.0")];
        assert_eq!(
            RegistryClient::max_version(vs.iter()),
            Some(s("2.10.0"))
        );
    }

    #[test]
    fn max_version_falls_back_to_string_for_unparseable() {
        let vs = vec![s("not-semver-b"), s("not-semver-a")];
        assert_eq!(
            RegistryClient::max_version(vs.iter()),
            Some(s("not-semver-b"))
        );
    }

    async fn mock_login(server: &MockServer) {
        Mock::given(method("PUT"))
            .and(path("/-/user/org.couchdb.user:cache-manager"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({"token":"tok"})),
            )
            .mount(server)
            .await;
    }

    fn packument() -> serde_json::Value {
        json!({
            "_rev": "1-abc",
            "name": "left-pad",
            "dist-tags": { "latest": "2.0.0" },
            "versions": {
                "1.0.0": { "name": "left-pad", "version": "1.0.0" },
                "2.0.0": { "name": "left-pad", "version": "2.0.0" }
            },
            "time": { "1.0.0": "t1", "2.0.0": "t2" }
        })
    }

    #[tokio::test]
    async fn unpublish_single_version_puts_metadata_and_deletes_tarball() {
        let server = MockServer::start().await;
        mock_login(&server).await;
        Mock::given(method("GET"))
            .and(path("/left-pad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(packument()))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(r"^/left-pad/-rev/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path_regex(r"^/left-pad/-/left-pad-1\.0\.0\.tgz/-rev/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = RegistryClient::new(&server.uri());
        client
            .unpublish_package("left-pad", Some("1.0.0"))
            .await
            .expect("unpublish should succeed");
    }

    #[tokio::test]
    async fn unpublish_missing_package_is_idempotent() {
        let server = MockServer::start().await;
        mock_login(&server).await;
        Mock::given(method("GET"))
            .and(path("/gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = RegistryClient::new(&server.uri());
        client
            .unpublish_package("gone", Some("1.0.0"))
            .await
            .expect("missing package should be treated as already removed");
    }

    #[tokio::test]
    async fn unpublish_whole_package_issues_delete() {
        let server = MockServer::start().await;
        mock_login(&server).await;
        Mock::given(method("GET"))
            .and(path("/left-pad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(packument()))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path_regex(r"^/left-pad/-rev/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = RegistryClient::new(&server.uri());
        client
            .unpublish_package("left-pad", None)
            .await
            .expect("whole-package unpublish should succeed");
    }

    #[tokio::test]
    async fn deprecate_puts_updated_metadata() {
        let server = MockServer::start().await;
        mock_login(&server).await;
        Mock::given(method("GET"))
            .and(path("/left-pad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(packument()))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/left-pad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(1)
            .mount(&server)
            .await;

        let client = RegistryClient::new(&server.uri());
        client
            .deprecate_package("left-pad", "1.0.0", "please upgrade")
            .await
            .expect("deprecate should succeed");
    }
}
