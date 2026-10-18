use crate::install_preflight::{check_install, PreflightReport};
use crate::local_scanner::{parse_tgz_metadata, LocalPackage};
use crate::replenishment::{read_list, ReplenishmentList};
use serde::Serialize;
use std::path::Path;

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
}

/// 旧导出目录只有 .tgz，仍可直接导入。
pub fn inspect(directory: &Path) -> Result<BundleInspection, String> {
    let list_path = directory.join("replenishment.json");
    let replenishment = if list_path.exists() { Some(read_list(&list_path)?) } else { None };
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()).is_none_or(|s| !s.eq_ignore_ascii_case("tgz")) { continue; }
        let (name, version, error) = match parse_tgz_metadata(&path) {
            Ok(LocalPackage { name, version, .. }) => (name, version, None),
            Err(e) => (path.file_name().unwrap_or_default().to_string_lossy().into(), String::new(), Some(e)),
        };
        files.push(BundleFile { name, version, path: path.to_string_lossy().into(), error });
    }
    files.sort_by(|a,b| a.path.cmp(&b.path));
    Ok(BundleInspection { files, replenishment })
}

pub async fn verify_replenishment(list: &ReplenishmentList, registry: &str) -> Result<PreflightReport, String> {
    check_install(list.dependencies(), registry, false, None, |_,_,_|{}).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_preflight::Availability;
    use crate::replenishment::{write_list, ReplenishmentEntry};
    use serde_json::json;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::path;

    #[test]
    fn accepts_legacy_directory_without_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.txt"), "legacy").unwrap();
        let inspection = inspect(dir.path()).unwrap();
        assert!(inspection.replenishment.is_none());
        assert!(inspection.files.is_empty());
    }

    #[tokio::test]
    async fn verifies_every_requested_entry_including_files_missing_from_directory() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let list = ReplenishmentList { kind: "verdaccio-replenishment".into(), schema_version: 1,
            generated_at: "2026-10-19T00:00:00Z".into(), source_registry: "http://intranet".into(),
            entries: vec!["present", "absent"].into_iter().map(|name| ReplenishmentEntry {
                name: name.into(), version: "1.0.0".into(), reason: "missing_package".into(),
            }).collect() };
        write_list(&dir.path().join("replenishment.json"), &list).unwrap();
        Mock::given(path("/-/cached-packages")).respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"name":"present","cached_versions":["1.0.0"]}
        ]))).mount(&server).await;
        Mock::given(path("/present")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":{"1.0.0":{}}}))).mount(&server).await;
        let inspection = inspect(dir.path()).unwrap();
        let report = verify_replenishment(&inspection.replenishment.unwrap(), &server.uri()).await.unwrap();
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].status, Availability::MissingPackage);
        assert_eq!(report.items[1].status, Availability::Available);
    }
}
