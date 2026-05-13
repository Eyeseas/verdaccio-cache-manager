use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPackage {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
}

pub fn scan_node_modules(project_dir: &Path) -> Result<Vec<LocalPackage>, String> {
    scan_node_modules_with_progress(project_dir, |_, _| {})
}

pub fn scan_node_modules_with_progress<F>(
    project_dir: &Path,
    mut on_progress: F,
) -> Result<Vec<LocalPackage>, String>
where
    F: FnMut(usize, &LocalPackage),
{
    let node_modules = if project_dir.file_name().and_then(|s| s.to_str()) == Some("node_modules") {
        project_dir.to_path_buf()
    } else {
        project_dir.join("node_modules")
    };

    if !node_modules.exists() {
        return Err(format!(
            "未找到 node_modules 目录（已检查 {}）",
            node_modules.display()
        ));
    }

    let mut packages = Vec::new();
    scan_dir(&node_modules, &mut packages, &mut on_progress)?;
    Ok(packages)
}

fn scan_dir<F>(
    dir: &Path,
    packages: &mut Vec<LocalPackage>,
    on_progress: &mut F,
) -> Result<(), String>
where
    F: FnMut(usize, &LocalPackage),
{
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if name.starts_with('@') {
            scan_dir(&path, packages, on_progress)?;
            continue;
        }

        let pkg_json = path.join("package.json");
        if pkg_json.exists() {
            if let Ok(content) = std::fs::read_to_string(&pkg_json) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    let pkg_name = json["name"].as_str().unwrap_or(&name).to_string();
                    let version = json["version"].as_str().unwrap_or("0.0.0").to_string();
                    let pkg = LocalPackage {
                        name: pkg_name,
                        version,
                        path: path.clone(),
                    };
                    packages.push(pkg.clone());
                    on_progress(packages.len(), &pkg);
                }
            }
        }
    }

    Ok(())
}

pub fn parse_tgz_metadata(tgz_path: &Path) -> Result<LocalPackage, String> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    use tar::Archive;

    let file = std::fs::File::open(tgz_path).map_err(|e| e.to_string())?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);

    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();

        if path.ends_with("package.json") {
            let mut content = String::new();
            entry.read_to_string(&mut content).map_err(|e| e.to_string())?;
            let json: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| e.to_string())?;

            return Ok(LocalPackage {
                name: json["name"].as_str().unwrap_or("unknown").to_string(),
                version: json["version"].as_str().unwrap_or("0.0.0").to_string(),
                path: tgz_path.to_path_buf(),
            });
        }
    }

    Err("tarball 中未找到 package.json".to_string())
}
