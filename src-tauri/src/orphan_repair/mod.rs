//! 孤儿 tarball：tgz 在 storage 里，元数据却没有登记该版本，npm 因此看不到它。
//!
//! 成因是 Verdaccio 非原子的 publish：先写 tgz，再改 package.json 登记
//! `_attachments` 和版本，后两步失败（同包并发发布时常见）就会留下孤儿。
//! 未登记 `_attachments` 的孤儿既无法通过 API 删除，也无法原名重发（409）。

use base64::Engine as _;
use sha1::Digest as _;

use crate::registry_client::{tarball_filename, RegistryClient};

/// 修复用附件名前缀。不能沿用 `<name>-<version>.tgz` 形式，否则按文件名
/// 解析版本的扫描（cached-list 插件、本地 storage 扫描）会多出一个伪版本。
pub const REPAIR_PREFIX: &str = "vcm-repair-";

/// 把磁盘上的 tarball 版本分成（已登记, 孤儿）两组，保持原有顺序。
pub fn split_orphans(versions: &[String], files: Vec<String>) -> (Vec<String>, Vec<String>) {
    files.into_iter().partition(|v| versions.contains(v))
}

pub fn shasum(data: &[u8]) -> String {
    sha1::Sha1::digest(data)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

pub fn integrity(data: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(data))
    )
}

/// 由 tarball 内容生成版本元数据：沿用包内 package.json（保留依赖信息），
/// 补上 dist。`tarball_url` 只有文件名部分有意义，Verdaccio 输出时会按请求 host 重写。
pub fn version_metadata(name: &str, version: &str, data: &[u8], tarball_url: String) -> serde_json::Value {
    let mut metadata = match crate::task_engine::extract_package_json_from_tarball(data) {
        Ok(mut pkg) if pkg.is_object() => {
            pkg["name"] = serde_json::json!(name);
            pkg["version"] = serde_json::json!(version);
            pkg
        }
        _ => serde_json::json!({ "name": name, "version": version }),
    };
    metadata["dist"] = serde_json::json!({
        "tarball": tarball_url,
        "shasum": shasum(data),
        "integrity": integrity(data),
    });
    metadata
}

/// 校验文件确实是 `name@version` 的包，防止把内容错乱的文件登记成该版本。
pub fn verify_identity(name: &str, version: &str, data: &[u8]) -> Result<(), String> {
    let pkg = crate::task_engine::extract_package_json_from_tarball(data)
        .map_err(|e| format!("无法解析 tarball: {}", e))?;
    if pkg["name"].as_str() != Some(name) || pkg["version"].as_str() != Some(version) {
        return Err(format!(
            "tarball 内容为 {}@{}，与文件名不符",
            pkg["name"].as_str().unwrap_or("?"),
            pkg["version"].as_str().unwrap_or("?")
        ));
    }
    Ok(())
}

/// 以 `vcm-repair-` 附件名发布来登记版本；原孤儿文件保留，
/// lockfile 里按原 URL 下载内容相同，integrity 校验仍能通过。
pub async fn publish_under_repair_name(
    client: &RegistryClient,
    name: &str,
    version: &str,
    mut metadata: serde_json::Value,
    data: &[u8],
) -> Result<(), String> {
    let repair_name = format!("{}{}", REPAIR_PREFIX, tarball_filename(name, version));
    metadata["dist"]["tarball"] = serde_json::json!(format!(
        "{}/{}/-/{}",
        client.registry_url.trim_end_matches('/'),
        name,
        repair_name
    ));
    client
        .publish_package_as(name, version, metadata, data, &repair_name)
        .await
        .map_err(|e| format!("修复孤儿 tarball 失败: {}", e))
}

/// 从目标 registry 读回孤儿文件并登记。不删除原文件：它可能是内网唯一一份。
pub async fn repair_from_registry(client: &RegistryClient, name: &str, version: &str) -> Result<(), String> {
    if client.has_version(name, version).await? {
        return Ok(());
    }
    let filename = tarball_filename(name, version);
    let data = client
        .fetch_tarball_file(name, &filename)
        .await?
        .ok_or_else(|| format!("服务端不存在文件 {}", filename))?;
    verify_identity(name, version, &data)?;
    let metadata = version_metadata(name, version, &data, String::new());
    publish_under_repair_name(client, name, version, metadata, &data).await
}

/// 测试用：只含 package.json 的最小 npm tarball。
#[cfg(test)]
pub(crate) fn test_tarball(name: &str, version: &str) -> Vec<u8> {
    let manifest = serde_json::json!({ "name": name, "version": version, "dependencies": { "dep": "^1" } })
        .to_string();
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "package/package.json", manifest.as_bytes())
        .unwrap();
    let tar = builder.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, &tar).unwrap();
    gz.finish().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn splits_registered_and_orphan_versions() {
        let versions = vec!["1.0.0".to_string(), "3.0.0".to_string()];
        let files = vec!["1.0.0".to_string(), "2.0.0".to_string()];
        assert_eq!(
            split_orphans(&versions, files),
            (vec!["1.0.0".to_string()], vec!["2.0.0".to_string()])
        );
    }

    #[test]
    fn rejects_tarball_with_other_identity() {
        let data = test_tarball("left-pad", "1.0.1");
        assert!(verify_identity("left-pad", "1.0.1", &data).is_ok());
        assert!(verify_identity("left-pad", "1.0.0", &data).is_err());
    }

    #[tokio::test]
    async fn repairs_orphan_from_registry_without_deleting_it() {
        let server = MockServer::start().await;
        let data = test_tarball("left-pad", "1.0.0");
        Mock::given(method("PUT"))
            .and(path("/-/user/org.couchdb.user:cache-manager"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "token": "tok" })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/left-pad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "versions": { "0.9.0": {} } })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/left-pad/-/left-pad-1.0.0.tgz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/left-pad"))
            .and(body_partial_json(json!({
                "versions": { "1.0.0": {
                    "dependencies": { "dep": "^1" },
                    "dist": {
                        "tarball": format!("{}/left-pad/-/vcm-repair-left-pad-1.0.0.tgz", server.uri()),
                        "integrity": integrity(&data)
                    }
                } },
                "_attachments": { "vcm-repair-left-pad-1.0.0.tgz": {} }
            })))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let client = RegistryClient::new(&server.uri());
        repair_from_registry(&client, "left-pad", "1.0.0").await.unwrap();
    }
}
