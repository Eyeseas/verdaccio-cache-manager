use crate::install_preflight::{check_install, PreflightReport};
use crate::local_scanner::{parse_tgz_metadata, LocalPackage};
use crate::registry_client::RegistryClient;
use crate::replenishment::validate_package;
use crate::replenishment::{read_list, ReplenishmentList};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub name: String,
    pub version: String,
    pub file_name: String,
    pub integrity: Option<String>,
    pub shasum: Option<String>,
    pub expected_latest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFailure {
    pub name: String,
    pub version: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub kind: String,
    pub schema_version: u32,
    /// Unix 毫秒时间戳。
    pub generated_at: u64,
    pub entries: Vec<ManifestEntry>,
    #[serde(default)]
    pub failures: Vec<ManifestFailure>,
}

impl BundleManifest {
    pub fn new() -> Self {
        Self {
            kind: "verdaccio-offline-bundle".into(),
            schema_version: 1,
            generated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            entries: vec![],
            failures: vec![],
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.kind != "verdaccio-offline-bundle" || self.schema_version != 1 {
            return Err("不支持的离线包 manifest 格式或 schema_version".into());
        }
        let mut packages = HashSet::new();
        let mut files = HashSet::new();
        let mut latest = HashMap::new();
        for entry in &self.entries {
            validate_package(&entry.name, &entry.version)?;
            let mut parts = Path::new(&entry.file_name).components();
            if !matches!(parts.next(), Some(Component::Normal(_)))
                || parts.next().is_some()
                || entry.file_name.contains(['/', '\\', ':'])
                || !entry.file_name.ends_with(".tgz")
            {
                return Err(format!(
                    "manifest 文件名必须是目录内的 .tgz: {}",
                    entry.file_name
                ));
            }
            if !packages.insert((&entry.name, &entry.version))
                || !files.insert(entry.file_name.to_ascii_lowercase())
            {
                return Err("manifest 包版本或文件名重复".into());
            }
            if entry.integrity.as_deref().unwrap_or("").is_empty()
                && entry.shasum.as_deref().unwrap_or("").is_empty()
            {
                return Err(format!("manifest 缺少完整性摘要: {}", entry.file_name));
            }
            if let Some(version) = &entry.expected_latest {
                validate_package(&entry.name, version)?;
            }
            if latest
                .insert(&entry.name, &entry.expected_latest)
                .is_some_and(|previous| previous != &entry.expected_latest)
            {
                return Err(format!(
                    "manifest 对 {} 声明了不同的 expected_latest",
                    entry.name
                ));
            }
        }
        Ok(())
    }
}

impl Default for BundleManifest {
    fn default() -> Self {
        Self::new()
    }
}

pub fn read_manifest(directory: &Path) -> Result<Option<BundleManifest>, String> {
    let path = directory.join("manifest.json");
    if !path.exists() {
        return Ok(None);
    }
    let manifest: BundleManifest =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("解析 manifest 失败: {}", e))?;
    manifest.validate()?;
    Ok(Some(manifest))
}

pub fn integrity(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(bytes))
    )
}

pub fn shasum(bytes: &[u8]) -> String {
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// SRI 有多个摘要时只接受最强算法，避免弱摘要掩盖损坏的强摘要。
pub fn verify_integrity(bytes: &[u8], sri: Option<&str>, sha1: Option<&str>) -> Result<(), String> {
    if let Some(sri) = sri {
        let tokens: Vec<_> = sri
            .split_whitespace()
            .filter_map(|token| token.split_once('-'))
            .collect();
        let strongest = ["sha512", "sha384", "sha256", "sha1"]
            .into_iter()
            .find(|algorithm| tokens.iter().any(|(a, _)| a == algorithm))
            .ok_or("不支持的 integrity 算法")?;
        let digest = match strongest {
            "sha512" => sha2::Sha512::digest(bytes).to_vec(),
            "sha384" => sha2::Sha384::digest(bytes).to_vec(),
            "sha256" => sha2::Sha256::digest(bytes).to_vec(),
            _ => sha1::Sha1::digest(bytes).to_vec(),
        };
        let expected = base64::engine::general_purpose::STANDARD.encode(digest);
        if !tokens.iter().any(|(algorithm, value)| {
            *algorithm == strongest && value.split('?').next() == Some(expected.as_str())
        }) {
            return Err("integrity 不匹配，文件可能损坏".into());
        }
    }
    if let Some(expected) = sha1 {
        if !shasum(bytes).eq_ignore_ascii_case(expected) {
            return Err("shasum 不匹配，文件可能损坏".into());
        }
    }
    Ok(())
}

pub fn validate_selected_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let Some(manifest) = read_manifest(path.parent().ok_or("文件缺少父目录")?)? else {
        return Ok(());
    };
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("无效文件名")?;
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.file_name == filename)
        .ok_or("文件不在 manifest 清单中")?;
    validate_entry(entry, bytes)
}

fn validate_entry(entry: &ManifestEntry, bytes: &[u8]) -> Result<(), String> {
    verify_integrity(bytes, entry.integrity.as_deref(), entry.shasum.as_deref())?;
    let metadata = crate::task_engine::extract_package_json_from_tarball(bytes)?;
    if metadata["name"].as_str() != Some(&entry.name)
        || metadata["version"].as_str() != Some(&entry.version)
    {
        return Err("tarball 的 name/version 与 manifest 不一致".into());
    }
    Ok(())
}

#[derive(Serialize)]
pub struct BundleFile {
    pub name: String,
    pub version: String,
    pub path: String,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct BundleInspection {
    pub files: Vec<BundleFile>,
    pub replenishment: Option<ReplenishmentList>,
    pub manifest: Option<BundleManifest>,
}

/// 旧导出目录只有 .tgz，仍可直接导入。
pub fn inspect(directory: &Path) -> Result<BundleInspection, String> {
    let list_path = directory.join("replenishment.json");
    let replenishment = if list_path.exists() {
        Some(read_list(&list_path)?)
    } else {
        None
    };
    let manifest = read_manifest(directory)?;
    let mut files = Vec::new();
    if let Some(manifest) = &manifest {
        for entry in &manifest.entries {
            let path = directory.join(&entry.file_name);
            let error = std::fs::read(&path)
                .map_err(|e| format!("读取文件失败: {}", e))
                .and_then(|bytes| validate_entry(entry, &bytes))
                .err();
            files.push(BundleFile {
                name: entry.name.clone(),
                version: entry.version.clone(),
                path: path.to_string_lossy().into(),
                error,
            });
        }
    }
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_none_or(|s| !s.eq_ignore_ascii_case("tgz"))
        {
            continue;
        }
        if let Some(manifest) = &manifest {
            if !manifest
                .entries
                .iter()
                .any(|e| Some(e.file_name.as_str()) == path.file_name().and_then(|s| s.to_str()))
            {
                files.push(BundleFile {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    version: String::new(),
                    path: path.to_string_lossy().into(),
                    error: Some("文件不在 manifest 清单中，不上传".into()),
                });
            }
            continue;
        }
        let (name, version, error) = match parse_tgz_metadata(&path) {
            Ok(LocalPackage { name, version, .. }) => (name, version, None),
            Err(e) => (
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                String::new(),
                Some(e),
            ),
        };
        files.push(BundleFile {
            name,
            version,
            path: path.to_string_lossy().into(),
            error,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(BundleInspection {
        files,
        replenishment,
        manifest,
    })
}

pub async fn verify_replenishment(
    list: &ReplenishmentList,
    registry: &str,
) -> Result<PreflightReport, String> {
    check_install(list.dependencies(), registry, false, None, |_, _, _| {}).await
}

#[derive(Debug, Serialize)]
pub struct LatestOutcome {
    pub name: String,
    pub expected_latest: String,
    pub actual_latest: Option<String>,
    pub action: String,
    pub message: Option<String>,
}

pub async fn apply_expected_latest(
    manifest: &BundleManifest,
    registry: &str,
) -> Vec<LatestOutcome> {
    let expected: std::collections::BTreeMap<_, _> = manifest
        .entries
        .iter()
        .filter_map(|e| {
            e.expected_latest
                .as_ref()
                .map(|v| (e.name.clone(), v.clone()))
        })
        .collect();
    let client = RegistryClient::new(registry);
    let mut outcomes = Vec::new();
    // 一次预检所有期望版本，确认 tgz 可用后才允许推进标签。
    let dependencies = expected
        .iter()
        .map(|(name, version)| crate::parser::ParsedDependency {
            name: name.clone(),
            version: version.clone(),
            tarball_url: None,
        })
        .collect();
    if expected.is_empty() {
        return outcomes;
    }
    let availability = check_install(dependencies, registry, false, None, |_, _, _| {}).await;
    for (name, version) in expected {
        let available = availability.as_ref().ok().is_some_and(|report| {
            report.items.iter().any(|item| {
                item.name == name
                    && item.status == crate::install_preflight::Availability::Available
            })
        });
        let mut outcome = LatestOutcome {
            name: name.clone(),
            expected_latest: version.clone(),
            actual_latest: None,
            action: "failed".into(),
            message: None,
        };
        let operation = async {
            if !available {
                return Err("期望 latest 版本不可下载或未能验证，未推进标签".to_string());
            }
            let changed = client.promote_latest(&name, &version).await?;
            let doc = client.fetch_packument(&name).await?;
            Ok((
                changed,
                doc["dist-tags"]["latest"].as_str().map(str::to_string),
            ))
        };
        match tokio::time::timeout(std::time::Duration::from_secs(60), operation).await {
            Ok(Ok((changed, actual))) => {
                outcome.action = if changed { "promoted" } else { "kept" }.into();
                if actual.as_deref() != Some(&version) {
                    outcome.message =
                        Some("目标 latest 与清单不同；保留当前标签，只前进不回退".into());
                }
                outcome.actual_latest = actual;
            }
            Ok(Err(error)) => outcome.message = Some(error),
            Err(_) => outcome.message = Some("推进 latest 超时".into()),
        }
        outcomes.push(outcome);
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_preflight::Availability;
    use crate::replenishment::{write_list, ReplenishmentEntry};
    use serde_json::json;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn tgz(name: &str, version: &str) -> Vec<u8> {
        let content = serde_json::to_vec(&json!({"name":name,"version":version})).unwrap();
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, "package/package.json", content.as_slice())
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap()
    }

    fn entry(bytes: &[u8]) -> ManifestEntry {
        ManifestEntry {
            name: "a".into(),
            version: "1.0.0".into(),
            file_name: "a-1.0.0.tgz".into(),
            integrity: Some(integrity(bytes)),
            shasum: Some(shasum(bytes)),
            expected_latest: Some("1.0.0".into()),
        }
    }

    #[test]
    fn sri_uses_strongest_algorithm_and_rejects_corruption() {
        let bytes = b"original";
        assert!(verify_integrity(bytes, Some(&integrity(bytes)), Some(&shasum(bytes))).is_ok());
        let weak = base64::engine::general_purpose::STANDARD.encode(sha1::Sha1::digest(bytes));
        assert!(
            verify_integrity(bytes, Some(&format!("sha1-{} sha512-wrong", weak)), None).is_err()
        );
        assert!(verify_integrity(b"corrupted", Some(&integrity(bytes)), None).is_err());
        assert!(verify_integrity(bytes, Some("unknown-abc"), None).is_err());
        assert!(verify_integrity(bytes, None, Some("wrong")).is_err());
    }

    #[test]
    fn manifest_validates_schema_paths_duplicates_and_latest() {
        let valid = entry(b"a");
        let mut manifest = BundleManifest::new();
        manifest.entries.push(valid.clone());
        assert!(manifest.validate().is_ok());
        for filename in ["../escape.tgz", "C:\\escape.tgz", "a/b.tgz", "a:b.tgz"] {
            manifest.entries[0].file_name = filename.into();
            assert!(manifest.validate().is_err());
        }
        manifest.entries = vec![valid.clone(), valid.clone()];
        assert!(manifest.validate().is_err());
        manifest.entries = vec![valid.clone()];
        manifest.schema_version = 2;
        assert!(manifest.validate().is_err());
        manifest.schema_version = 1;
        manifest.entries[0].expected_latest = Some("^1".into());
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn inspection_rejects_corrupt_missing_extra_and_mismatched_files() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = tgz("a", "1.0.0");
        let mut manifest = BundleManifest::new();
        manifest.entries.push(entry(&bytes));
        let path = dir.path().join("a-1.0.0.tgz");
        let save = |manifest: &BundleManifest| {
            std::fs::write(
                dir.path().join("manifest.json"),
                serde_json::to_vec(manifest).unwrap(),
            )
            .unwrap()
        };
        save(&manifest);
        assert!(inspect(dir.path()).unwrap().files[0].error.is_some());
        std::fs::write(&path, &bytes).unwrap();
        assert!(inspect(dir.path()).unwrap().files[0].error.is_none());
        let other = tgz("b", "1.0.0");
        std::fs::write(&path, &other).unwrap();
        assert!(validate_selected_file(&path, &other)
            .unwrap_err()
            .contains("integrity"));
        manifest.entries[0].integrity = Some(integrity(&other));
        manifest.entries[0].shasum = Some(shasum(&other));
        save(&manifest);
        assert!(validate_selected_file(&path, &other)
            .unwrap_err()
            .contains("name/version"));
        std::fs::write(dir.path().join("extra.tgz"), &bytes).unwrap();
        let inspection = inspect(dir.path()).unwrap();
        assert_eq!(inspection.files.len(), 2);
        assert!(inspection.files.iter().all(|f| f.error.is_some()));
    }

    #[tokio::test]
    async fn export_writes_manifest_with_actual_hashes_latest_and_partial_failures() {
        use crate::registry_client::download_tarballs_from_registry;
        use wiremock::matchers::method;
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let bytes = tgz("@s/a", "1.0.0");
        Mock::given(path("/@s%2fa"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "dist-tags":{"latest":"2.0.0"},"versions":{"1.0.0":{"dist":{
                    "tarball":format!("{}/custom.tgz",server.uri()), "integrity":integrity(&bytes)
                }}}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/custom.tgz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.clone()))
            .mount(&server)
            .await;
        let result = download_tarballs_from_registry(
            &[
                ("@s/a".into(), "1.0.0".into()),
                ("missing".into(), "1.0.0".into()),
            ],
            dir.path(),
            &server.uri(),
            |_, _, _| {},
        )
        .await
        .unwrap();
        assert_eq!(result.success, 1);
        assert_eq!(result.failed, 1);
        let manifest = read_manifest(dir.path()).unwrap().unwrap();
        assert_eq!(manifest.entries[0].file_name, "@s-a-1.0.0.tgz");
        assert_eq!(
            manifest.entries[0].expected_latest.as_deref(),
            Some("2.0.0")
        );
        assert_eq!(
            manifest.entries[0].integrity.as_deref(),
            Some(integrity(&bytes).as_str())
        );
        assert_eq!(manifest.failures.len(), 1);
        assert!(inspect(dir.path()).unwrap().files[0].error.is_none());
    }

    #[tokio::test]
    async fn latest_promotes_only_available_versions_and_never_rolls_back() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        use wiremock::matchers::method;
        let server = MockServer::start().await;
        let promoted = Arc::new(AtomicBool::new(false));
        Mock::given(path("/-/cached-packages")).respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"name":"a","cached_versions":["1.0.0"]}, {"name":"b","cached_versions":["1.0.0","2.0.0"]}, {"name":"c","cached_versions":[]}
        ]))).mount(&server).await;
        for name in ["a", "b", "c"] {
            let changed = promoted.clone();
            Mock::given(method("GET"))
                .and(path(format!("/{}", name)))
                .respond_with(move |_: &wiremock::Request| {
                    let latest = if name == "b" {
                        "2.0.0"
                    } else if changed.load(Ordering::SeqCst) {
                        "1.0.0"
                    } else {
                        "0.9.0"
                    };
                    ResponseTemplate::new(200).set_body_json(
                        json!({"dist-tags":{"latest":latest}, "versions":{"1.0.0":{},"2.0.0":{}}}),
                    )
                })
                .mount(&server)
                .await;
        }
        Mock::given(method("PUT"))
            .and(path("/-/user/org.couchdb.user:cache-manager"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"token":"test"})))
            .mount(&server)
            .await;
        let changed = promoted.clone();
        Mock::given(method("PUT"))
            .and(path("/-/package/a/dist-tags/latest"))
            .respond_with(move |_: &wiremock::Request| {
                changed.store(true, Ordering::SeqCst);
                ResponseTemplate::new(201)
            })
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/-/package/b/dist-tags/latest"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/-/package/c/dist-tags/latest"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let mut manifest = BundleManifest::new();
        for name in ["a", "b", "c"] {
            let mut e = entry(b"a");
            e.name = name.into();
            e.file_name = format!("{}-1.0.0.tgz", name);
            manifest.entries.push(e);
        }
        let outcomes = apply_expected_latest(&manifest, &server.uri()).await;
        assert_eq!(outcomes[0].action, "promoted");
        assert_eq!(outcomes[0].actual_latest.as_deref(), Some("1.0.0"));
        assert_eq!(outcomes[1].action, "kept");
        assert_eq!(outcomes[1].actual_latest.as_deref(), Some("2.0.0"));
        assert_eq!(outcomes[2].action, "failed");
    }

    #[test]
    fn accepts_legacy_directory_without_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.txt"), "legacy").unwrap();
        std::fs::write(dir.path().join("a-1.0.0.tgz"), tgz("a", "1.0.0")).unwrap();
        let inspection = inspect(dir.path()).unwrap();
        assert!(inspection.replenishment.is_none());
        assert!(inspection.manifest.is_none());
        assert_eq!(inspection.files.len(), 1);
        assert!(inspection.files[0].error.is_none());
    }

    #[tokio::test]
    async fn verifies_every_requested_entry_including_files_missing_from_directory() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let list = ReplenishmentList {
            kind: "verdaccio-replenishment".into(),
            schema_version: 1,
            generated_at: "2026-10-19T00:00:00Z".into(),
            source_registry: "http://intranet".into(),
            entries: vec!["present", "absent"]
                .into_iter()
                .map(|name| ReplenishmentEntry {
                    name: name.into(),
                    version: "1.0.0".into(),
                    reason: "missing_package".into(),
                })
                .collect(),
        };
        write_list(&dir.path().join("replenishment.json"), &list).unwrap();
        Mock::given(path("/-/cached-packages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"name":"present","cached_versions":["1.0.0"]}
            ])))
            .mount(&server)
            .await;
        Mock::given(path("/present"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"versions":{"1.0.0":{}}})),
            )
            .mount(&server)
            .await;
        let inspection = inspect(dir.path()).unwrap();
        let report = verify_replenishment(&inspection.replenishment.unwrap(), &server.uri())
            .await
            .unwrap();
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].status, Availability::MissingPackage);
        assert_eq!(report.items[1].status, Availability::Available);
    }
}
