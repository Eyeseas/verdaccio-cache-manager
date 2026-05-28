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

        let components: Vec<_> = path.components().collect();
        let is_top_level_package_json = components.len() == 2
            && components
                .last()
                .and_then(|c| c.as_os_str().to_str())
                == Some("package.json");
        if is_top_level_package_json {
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

    Err("tarball 中未找到顶级 package.json，可能不是合法的 npm 包".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use tar::{Builder, Header};
    use tempfile::NamedTempFile;

    fn append_file(builder: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &[u8]) {
        let mut header = Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, content).unwrap();
    }

    fn write_tgz(entries: &[(&str, &[u8])]) -> NamedTempFile {
        let mut buf: Vec<u8> = Vec::new();
        {
            let encoder = GzEncoder::new(&mut buf, Compression::default());
            let mut builder = Builder::new(encoder);
            for (path, content) in entries {
                append_file(&mut builder, path, content);
            }
            builder.into_inner().unwrap().finish().unwrap();
        }

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&buf).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn parse_tgz_metadata_returns_root() {
        let root = br#"{"name":"my-pkg","version":"1.2.3"}"#;
        let file = write_tgz(&[("package/package.json", root)]);

        let pkg = parse_tgz_metadata(file.path()).unwrap();
        assert_eq!(pkg.name, "my-pkg");
        assert_eq!(pkg.version, "1.2.3");
    }

    #[test]
    fn parse_tgz_metadata_skips_nested_package_json() {
        // 模拟 throttle-debounce 这类 dual ESM/CJS 包：
        // 嵌套的 esm/package.json 先于根 package.json 出现在 tar 流中。
        let nested = br#"{"type":"module"}"#;
        let root = br#"{"name":"throttle-debounce","version":"5.0.2"}"#;
        let file = write_tgz(&[
            ("package/esm/package.json", nested),
            ("package/package.json", root),
        ]);

        let pkg = parse_tgz_metadata(file.path()).unwrap();
        assert_eq!(pkg.name, "throttle-debounce");
        assert_eq!(pkg.version, "5.0.2");
    }

    #[test]
    fn parse_tgz_metadata_errors_when_no_root_package_json() {
        let nested = br#"{"type":"module"}"#;
        let file = write_tgz(&[("package/esm/package.json", nested)]);

        let err = parse_tgz_metadata(file.path()).unwrap_err();
        assert!(err.contains("顶级 package.json"));
    }
}
