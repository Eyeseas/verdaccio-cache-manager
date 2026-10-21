//! Launched by `node scripts/verdaccio-e2e.mjs`; never touches a user's registry.
use flate2::{write::GzEncoder, Compression};
use serde_json::{json, Value};
use std::path::Path;
use verdaccio_cache_manager_lib::{
    registry_client::{tarball_url, RegistryClient},
    storage_health::{check_remote, check_storage, repair_packages, repair_remote, RepairAction},
};

fn archive(name: &str, version: &str) -> Vec<u8> {
    let content = json!({"name": name, "version": version}).to_string();
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "package/package.json", content.as_bytes())
        .unwrap();
    tar.into_inner().unwrap().finish().unwrap()
}

fn corrupt(storage: &Path, name: &str, empty: bool) -> Value {
    let path = storage.join(name).join("package.json");
    let mut doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut ghost = doc["versions"]["1.0.0"].clone();
    ghost["version"] = json!("9.0.0");
    ghost["dist"]["tarball"] = json!("http://127.0.0.1:1/ghost.tgz");
    if !name.ends_with("stale") {
        doc["versions"]["9.0.0"] = ghost;
        doc["dist-tags"]["next"] = json!("9.0.0");
    }
    doc["dist-tags"]["latest"] = json!(if empty || name.ends_with("ghost-latest") {
        "9.0.0"
    } else {
        "1.0.0"
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    if empty {
        for file in std::fs::read_dir(path.parent().unwrap()).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_some_and(|e| e == "tgz") {
                std::fs::remove_file(path).unwrap();
            }
        }
    }
    doc
}

#[tokio::test]
#[ignore = "real Verdaccio: node scripts/verdaccio-e2e.mjs"]
async fn real_verdaccio_health_and_repairs() {
    let url = std::env::var("VCM_E2E_URL").expect("use the isolated E2E launcher");
    let storage = std::env::var("VCM_E2E_STORAGE").unwrap();
    let storage = Path::new(&storage);
    let client = RegistryClient::new(&url);
    let http = reqwest::Client::new();
    let packages = [
        "vcm-e2e-plain",
        "@vcm-e2e/scoped",
        "vcm-e2e-empty",
        "@vcm-e2e/empty",
        "vcm-e2e-stale",
        "@vcm-e2e/ghost-latest",
    ];
    for name in packages {
        for version in ["1.0.0", "2.0.0"] {
            let data = archive(name, version);
            client
                .publish_package(
                    name,
                    version,
                    json!({
                        "name": name, "version": version,
                        "dist": {"tarball": tarball_url(&url, name, version)}
                    }),
                    &data,
                )
                .await
                .unwrap_or_else(|e| panic!("publish {name}@{version}: {e}"));
        }
    }
    println!("[e2e] published 12 versions, including scoped packages");
    for name in packages {
        corrupt(storage, name, name.ends_with("empty"));
    }

    let default_entries = client.fetch_plugin_entries(false).await.unwrap();
    let all_entries = client.fetch_plugin_entries(true).await.unwrap();
    assert_eq!(default_entries.len(), 4);
    assert_eq!(all_entries.len(), 6);
    for entry in &default_entries {
        assert_eq!(entry["cached_versions"], json!(["1.0.0", "2.0.0"]));
        let name = entry["name"].as_str().unwrap();
        assert_eq!(
            entry["versions"],
            if name.ends_with("stale") {
                json!(["1.0.0", "2.0.0"])
            } else {
                json!(["1.0.0", "2.0.0", "9.0.0"])
            }
        );
        assert_eq!(
            entry["dist_latest"],
            if name.ends_with("ghost-latest") {
                "9.0.0"
            } else {
                "1.0.0"
            }
        );
    }
    let local = check_storage(storage).unwrap();
    let remote = check_remote(&client).await.unwrap();
    assert_eq!(
        serde_json::to_value(&local).unwrap(),
        serde_json::to_value(&remote).unwrap()
    );
    assert_eq!(remote.unhealthy.len(), 6);
    assert!(!remote.plugin_outdated);
    println!(
        "[e2e] plugin default=4/include_empty=6; local and remote reports match (6 unhealthy)"
    );

    let outcomes = repair_remote(&client, remote.unhealthy).await;
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| o.action == RepairAction::Repaired)
            .count(),
        4,
        "{outcomes:?}"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| o.action == RepairAction::Removed)
            .count(),
        2,
        "{outcomes:?}"
    );
    assert!(check_remote(&client).await.unwrap().unhealthy.is_empty());
    assert!(check_storage(storage).unwrap().unhealthy.is_empty());
    for name in packages {
        let response = http.get(format!("{url}/{name}")).send().await.unwrap();
        if name.ends_with("empty") {
            assert_eq!(response.status(), 404);
            continue;
        }
        let doc: Value = response.error_for_status().unwrap().json().await.unwrap();
        assert!(doc["versions"].get("9.0.0").is_none());
        assert_eq!(doc["dist-tags"]["latest"], "2.0.0");
        assert!(doc["dist-tags"].get("next").is_none());
        for version in ["1.0.0", "2.0.0"] {
            let response = http
                .get(tarball_url(&url, name, version))
                .send()
                .await
                .unwrap();
            assert!(
                response.status().is_success(),
                "tarball {name}@{version}: {}",
                response.status()
            );
            assert_eq!(
                response.bytes().await.unwrap().as_ref(),
                archive(name, version)
            );
        }
        assert_eq!(
            http.get(tarball_url(&url, name, "9.0.0"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        // Set latest back deliberately, then exercise the production monotonic promotion API.
        http.put(format!(
            "{url}/-/package/{}/dist-tags/latest",
            name.replace('/', "%2f")
        ))
        .json(&"1.0.0")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
        assert!(client.promote_latest(name, "2.0.0").await.unwrap());
        assert!(!client.promote_latest(name, "1.0.0").await.unwrap());
        assert!(!client.promote_latest(name, "2.0.0").await.unwrap());
        let doc: Value = http
            .get(format!("{url}/{name}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(doc["dist-tags"]["latest"], "2.0.0");
    }
    println!("[e2e] remote repair healthy; empty packages/ghost tarballs=404; packuments and cached tarballs valid; latest only advances");

    // Local repair runs against a separate copy: live Verdaccio must be stopped before editing its storage.
    let local_storage = tempfile::tempdir().unwrap();
    let mut originals = Vec::new();
    for name in packages {
        let dir = local_storage.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let source = storage.join(if name.ends_with("empty") {
            packages[0]
        } else {
            name
        });
        for file in std::fs::read_dir(source).unwrap() {
            let file = file.unwrap();
            if file.path().is_file() {
                std::fs::copy(file.path(), dir.join(file.file_name())).unwrap();
            }
        }
        let path = dir.join("package.json");
        let mut doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        doc["name"] = json!(name);
        std::fs::write(&path, doc.to_string()).unwrap();
        originals.push(corrupt(local_storage.path(), name, name.ends_with("empty")));
    }
    let names: Vec<String> = packages.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        check_storage(local_storage.path()).unwrap().unhealthy.len(),
        6
    );
    let outcomes = repair_packages(local_storage.path(), &names, &url).unwrap();
    for (outcome, original) in outcomes.iter().zip(originals) {
        let manifest = local_storage
            .path()
            .join(&outcome.name)
            .join("package.json");
        if outcome.name.ends_with("empty") {
            assert_eq!(outcome.action, RepairAction::Removed, "{outcome:?}");
            assert!(!manifest.exists());
        } else {
            assert_eq!(outcome.action, RepairAction::Repaired, "{outcome:?}");
            let doc: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
            assert_eq!(doc["versions"].as_object().unwrap().len(), 2);
            assert!(doc["versions"].get("9.0.0").is_none());
            assert_eq!(doc["dist-tags"]["latest"], "2.0.0");
            assert!(doc["dist-tags"].get("next").is_none());
        }
        let backup = Path::new(outcome.backup_path.as_ref().unwrap());
        assert_eq!(backup.extension().unwrap(), "bak");
        let saved: Value = serde_json::from_slice(&std::fs::read(backup).unwrap()).unwrap();
        assert_eq!(saved, original);
    }
    assert!(check_storage(local_storage.path())
        .unwrap()
        .unhealthy
        .is_empty());
    println!("[e2e] local repair healthy; all 6 .bak backups preserve original metadata");

    // Orphan tarball: the file lands in storage but the packument never registers it,
    // as left behind by concurrent publishes of one package.
    let orphan = "vcm-e2e-orphan";
    client
        .publish_package(
            orphan,
            "1.0.0",
            json!({"name": orphan, "version": "1.0.0"}),
            &archive(orphan, "1.0.0"),
        )
        .await
        .unwrap();
    std::fs::write(
        storage.join(orphan).join(format!("{orphan}-3.0.0.tgz")),
        archive(orphan, "3.0.0"),
    )
    .unwrap();
    let listed = client.list_cached_via_plugin().await.unwrap();
    let entry = listed.iter().find(|p| p.name == orphan).unwrap();
    assert_eq!(entry.cached_versions, vec!["1.0.0"]);
    assert_eq!(entry.orphan_versions, vec!["3.0.0"]);
    let report = check_remote(&client).await.unwrap();
    assert_eq!(report.unhealthy.len(), 1);
    assert_eq!(report.unhealthy[0].orphan_versions, vec!["3.0.0"]);
    let outcomes = repair_remote(&client, report.unhealthy).await;
    assert_eq!(outcomes[0].action, RepairAction::Repaired, "{outcomes:?}");
    assert_eq!(outcomes[0].registered_versions, vec!["3.0.0"]);
    let doc: Value = http
        .get(format!("{url}/{orphan}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(doc["dist-tags"]["latest"], "3.0.0");
    let tarball = doc["versions"]["3.0.0"]["dist"]["tarball"].as_str().unwrap();
    for url in [tarball.to_string(), tarball_url(&url, orphan, "3.0.0")] {
        let bytes = http.get(&url).send().await.unwrap().error_for_status().unwrap().bytes().await.unwrap();
        assert_eq!(bytes.as_ref(), archive(orphan, "3.0.0"), "{url}");
    }
    assert!(check_remote(&client).await.unwrap().unhealthy.is_empty());
    println!("[e2e] orphan tarball detected, registered via API; metadata and original URLs both serve it");
}
