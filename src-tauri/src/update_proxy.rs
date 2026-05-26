use std::collections::HashMap;
use std::env;
#[cfg(target_os = "macos")]
use std::process::Command;

const PROXY_ENV_VARS: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];

#[tauri::command]
pub(crate) fn get_update_proxy() -> Option<String> {
    detect_update_proxy()
}

fn detect_update_proxy() -> Option<String> {
    proxy_from_environment().or_else(proxy_from_system)
}

fn proxy_from_environment() -> Option<String> {
    PROXY_ENV_VARS
        .iter()
        .filter_map(|name| env::var(name).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

#[cfg(target_os = "macos")]
fn proxy_from_system() -> Option<String> {
    let output = Command::new("scutil").arg("--proxy").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    proxy_from_scutil_output(&stdout)
}

#[cfg(not(target_os = "macos"))]
fn proxy_from_system() -> Option<String> {
    None
}

pub(crate) fn proxy_from_scutil_output(output: &str) -> Option<String> {
    let settings = parse_scutil_output(output);
    proxy_from_scutil_prefix(&settings, "HTTPS")
        .or_else(|| proxy_from_scutil_prefix(&settings, "HTTP"))
}

fn parse_scutil_output(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn proxy_from_scutil_prefix(settings: &HashMap<String, String>, prefix: &str) -> Option<String> {
    let enabled = settings
        .get(&format!("{prefix}Enable"))
        .map(|value| value == "1")
        .unwrap_or(false);
    if !enabled {
        return None;
    }

    let host = settings.get(&format!("{prefix}Proxy"))?;
    let port = settings.get(&format!("{prefix}Port"))?;
    format_http_proxy(host, port)
}

fn format_http_proxy(host: &str, port: &str) -> Option<String> {
    let host = host
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    if host.is_empty() {
        return None;
    }

    let port: u16 = port.trim().parse().ok()?;
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };

    Some(format!("http://{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scutil_proxy_prefers_enabled_https_proxy() {
        let output = r#"<dictionary> {
  HTTPEnable : 1
  HTTPPort : 8080
  HTTPProxy : proxy.example.test
  HTTPSEnable : 1
  HTTPSPort : 10808
  HTTPSProxy : proxy-secure.example.test
}"#;

        assert_eq!(
            proxy_from_scutil_output(output),
            Some("http://proxy-secure.example.test:10808".to_string())
        );
    }

    #[test]
    fn scutil_proxy_falls_back_to_http_proxy() {
        let output = r#"<dictionary> {
  HTTPEnable : 1
  HTTPPort : 8080
  HTTPProxy : proxy.example.test
  HTTPSEnable : 0
  HTTPSPort : 10808
  HTTPSProxy : proxy-secure.example.test
}"#;

        assert_eq!(
            proxy_from_scutil_output(output),
            Some("http://proxy.example.test:8080".to_string())
        );
    }

    #[test]
    fn scutil_proxy_ignores_disabled_or_incomplete_entries() {
        let output = r#"<dictionary> {
  HTTPEnable : 0
  HTTPPort : 8080
  HTTPProxy : proxy.example.test
  HTTPSEnable : 1
  HTTPSProxy : proxy-secure.example.test
}"#;

        assert_eq!(proxy_from_scutil_output(output), None);
    }
}
