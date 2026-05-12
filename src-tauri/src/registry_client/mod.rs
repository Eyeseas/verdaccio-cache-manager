use serde::{Deserialize, Serialize};

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
}

pub struct RegistryClient {
    http: reqwest::Client,
    pub registry_url: String,
    token: Option<String>,
}

impl RegistryClient {
    pub fn new(registry_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            registry_url: registry_url.to_string(),
            token: None,
        }
    }

    pub async fn ensure_token(&mut self) -> Result<(), String> {
        if self.token.is_some() {
            return Ok(());
        }

        let url = format!("{}/-/user/org.couchdb.user:cache-manager", self.registry_url);
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

        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        if let Some(token) = json["token"].as_str() {
            self.token = Some(token.to_string());
        } else if let Some(token) = json["ok"].as_str() {
            self.token = Some(token.to_string());
        } else {
            use base64::Engine;
            let basic = base64::engine::general_purpose::STANDARD
                .encode("cache-manager:cache-manager-auto");
            self.token = Some(format!("Basic {}", basic));
        }

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
        let name_part = if package_name.starts_with('@') {
            package_name.split('/').last().unwrap_or(package_name)
        } else {
            package_name
        };
        let tarball_url = format!(
            "{}/{}/-/{}-{}.tgz",
            self.registry_url, package_name, name_part, version
        );

        let resp = self
            .http
            .get(&tarball_url)
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

    pub async fn publish_package(
        &mut self,
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

        if let Some(ref token) = self.token {
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
            let text = resp.text().await.unwrap_or_default();
            Err(format!("Publish 失败 ({}): {}", status, text))
        }
    }
}
