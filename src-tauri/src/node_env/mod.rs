use std::process::Command;
use std::sync::OnceLock;

use crate::config::AppConfig;

/// 从 Finder / Dock 启动的 GUI 应用由 launchd 拉起，只继承
/// `/usr/bin:/bin:/usr/sbin:/sbin`，nvm / fnm / Homebrew 等写在 shell rc 中的
/// 路径都拿不到。这里通过用户登录 shell 读取完整 PATH，并与当前 PATH 合并。
/// 结果进程内缓存，只在首次需要时执行一次。
fn search_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let current = std::env::var("PATH").unwrap_or_default();
        match login_shell_path() {
            Some(shell_path) => merge_paths(&shell_path, &current),
            None => current,
        }
    })
}

/// 以 shell 的 PATH 为准（保持用户期望的优先级），再追加当前进程独有的条目。
fn merge_paths(primary: &str, secondary: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    primary
        .split(':')
        .chain(secondary.split(':'))
        .filter(|p| !p.is_empty() && seen.insert(*p))
        .collect::<Vec<_>>()
        .join(":")
}

const ENV_START: &str = "__VCM_ENV_START__";
const ENV_END: &str = "__VCM_ENV_END__";

/// 从 `env` 输出中取出 PATH。用标记包裹，避免 shell rc 打印的内容干扰。
fn extract_path_from_env_output(output: &str) -> Option<String> {
    let start = output.find(ENV_START)? + ENV_START.len();
    let end = start + output[start..].find(ENV_END)?;
    output[start..end]
        .lines()
        .find_map(|line| line.strip_prefix("PATH="))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

#[cfg(unix)]
fn login_shell_path() -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string());
    // nvm 通常只写在 .zshrc / .bashrc，需要 -i 才会加载；用 /usr/bin/env
    // 输出 PATH，兼容 fish 等 PATH 为列表的 shell。
    let script = format!("echo {ENV_START}; /usr/bin/env; echo {ENV_END}");
    let mut child = Command::new(&shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // 读取放在独立线程，防止输出过大填满管道导致子进程阻塞
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let output = reader.join().ok()?;
    extract_path_from_env_output(&output)
}

#[cfg(not(unix))]
fn login_shell_path() -> Option<String> {
    None
}

#[cfg(unix)]
fn node_command() -> Command {
    use std::os::unix::fs::PermissionsExt;

    let path = search_path();
    let binary = path
        .split(':')
        .map(|dir| std::path::Path::new(dir).join("node"))
        .find(|p| {
            std::fs::metadata(p)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        });
    let mut cmd = Command::new(binary.as_deref().unwrap_or(std::path::Path::new("node")));
    cmd.env("PATH", path);
    cmd
}

#[cfg(not(unix))]
fn node_command() -> Command {
    Command::new("node")
}

fn normalize_node_version_stdout(stdout: &[u8]) -> Result<String, String> {
    let version = String::from_utf8_lossy(stdout).trim().to_string();
    if version.is_empty() {
        Err("node -v 未返回版本".to_string())
    } else {
        Ok(version)
    }
}

/// 检测本机 Node 版本，返回 `node -v` 的原样输出（如 `v22.12.0`）。
/// 首次调用可能需要启动登录 shell，请勿在主线程调用。
pub fn detect_local_node_version() -> Result<String, String> {
    let output = node_command()
        .arg("-v")
        .output()
        .map_err(|e| format!("无法执行 node -v: {}", e))?;
    if !output.status.success() {
        return Err("node -v 执行失败".to_string());
    }
    normalize_node_version_stdout(&output.stdout)
}

/// 规范化用户填写的版本号：去掉首尾空白与前缀 `v`。
fn parse_node_version(raw: &str) -> Option<node_semver::Version> {
    let trimmed = raw.trim();
    let trimmed = trimmed.strip_prefix('v').unwrap_or(trimmed);
    node_semver::Version::parse(trimmed).ok()
}

/// 校验并规范化设置中的目标 Node 版本：空白视为未设置，统一为 `v` 前缀格式。
pub fn normalize_target_node_version(raw: Option<String>) -> Result<Option<String>, String> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => parse_node_version(s)
            .map(|v| Some(format!("v{}", v)))
            .ok_or_else(|| format!("目标 Node 版本无效: {}（示例: 18.20.4）", s)),
    }
}

/// 解析依赖时采用的 Node 版本（展示用字符串）：设置中指定了目标版本则用它，
/// 否则使用本机检测到的版本。
pub fn effective_node_version_label(config: &AppConfig) -> Result<String, String> {
    match config
        .target_node_version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(target) => parse_node_version(target)
            .map(|v| format!("v{}", v))
            .ok_or_else(|| format!("目标 Node 版本无效: {}", target)),
        None => detect_local_node_version(),
    }
}

/// 解析依赖时用于匹配 `engines.node` 的版本；均不可用时返回 None
/// （此时不按 engines 过滤）。
pub fn effective_node_version(config: &AppConfig) -> Option<node_semver::Version> {
    effective_node_version_label(config)
        .ok()
        .and_then(|v| parse_node_version(&v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_node_version_stdout_trims_version() {
        let version = normalize_node_version_stdout(b"v22.12.0\n").unwrap();
        assert_eq!(version, "v22.12.0");
    }

    #[test]
    fn normalize_node_version_stdout_rejects_empty_output() {
        let err = normalize_node_version_stdout(b" \n").unwrap_err();
        assert!(err.contains("未返回版本"));
    }

    #[test]
    fn extract_path_ignores_rc_noise() {
        let output = format!(
            "Welcome!\nPATH=/noise\n{ENV_START}\nHOME=/Users/a\nPATH=/Users/a/.nvm/versions/node/v24/bin:/usr/bin\n{ENV_END}\n"
        );
        assert_eq!(
            extract_path_from_env_output(&output).as_deref(),
            Some("/Users/a/.nvm/versions/node/v24/bin:/usr/bin")
        );
    }

    #[test]
    fn extract_path_requires_markers() {
        assert_eq!(extract_path_from_env_output("PATH=/usr/bin\n"), None);
    }

    #[test]
    fn merge_paths_keeps_primary_order_and_dedupes() {
        assert_eq!(
            merge_paths("/a:/usr/bin:/b", "/usr/bin:/bin:/a"),
            "/a:/usr/bin:/b:/bin"
        );
    }

    #[test]
    fn parse_node_version_accepts_v_prefix() {
        assert_eq!(parse_node_version(" v18.20.4 ").unwrap().to_string(), "18.20.4");
        assert_eq!(parse_node_version("20.0.0").unwrap().to_string(), "20.0.0");
        assert!(parse_node_version("18").is_none());
    }

    #[test]
    fn normalize_target_node_version_cases() {
        assert_eq!(normalize_target_node_version(None).unwrap(), None);
        assert_eq!(normalize_target_node_version(Some("  ".into())).unwrap(), None);
        assert_eq!(
            normalize_target_node_version(Some(" 18.20.4 ".into())).unwrap().as_deref(),
            Some("v18.20.4")
        );
        assert!(normalize_target_node_version(Some("18".into())).is_err());
    }

    #[test]
    fn effective_node_version_prefers_target() {
        let config = AppConfig {
            target_node_version: Some("v18.20.4".to_string()),
            ..AppConfig::default()
        };
        assert_eq!(effective_node_version_label(&config).unwrap(), "v18.20.4");
        assert_eq!(effective_node_version(&config).unwrap().to_string(), "18.20.4");
    }

    #[test]
    fn effective_node_version_rejects_invalid_target() {
        let config = AppConfig {
            target_node_version: Some("eighteen".to_string()),
            ..AppConfig::default()
        };
        assert!(effective_node_version_label(&config).is_err());
    }
}
