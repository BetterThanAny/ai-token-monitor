use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub fn expand_home_path(path: &str, home: &Path) -> Option<PathBuf> {
    let path = path.trim();
    if path.is_empty() {
        None
    } else if path == "~" {
        Some(home.to_path_buf())
    } else if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(home.join(rest))
    } else {
        Some(PathBuf::from(path))
    }
}

pub fn display_path(path: &Path, home: &Path) -> String {
    if let Ok(stripped) = path.strip_prefix(home) {
        let suffix = stripped.to_string_lossy();
        if suffix.is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", suffix)
        }
    } else {
        path.display().to_string()
    }
}

pub fn has_codex_sessions(path: &Path) -> bool {
    path.join("sessions").is_dir() || path.join("archived_sessions").is_dir()
}

fn push_unique(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
    if seen.insert(canonical) {
        paths.push(path);
    }
}

fn push_if_codex_dir(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if has_codex_sessions(&path) {
        push_unique(paths, seen, path);
    }
}

fn discover_home_named_codex_dirs(
    home: &Path,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    if let Ok(entries) = fs::read_dir(home) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".codex-") {
                push_if_codex_dir(paths, seen, entry.path());
            }
        }
    }
}

fn discover_env_codex_dirs(home: &Path, paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    for var in ["CODEX_CONFIG_DIR", "CODEX_HOME"] {
        if let Ok(value) = env::var(var) {
            if let Some(path) = expand_home_path(&value, home) {
                push_if_codex_dir(paths, seen, path);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn discover_wsl_distro_codex_dirs(
    distro_path: PathBuf,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    let distro_home = distro_path.join("home");
    if let Ok(users) = fs::read_dir(&distro_home) {
        for user in users.flatten() {
            let user_home = user.path();
            push_if_codex_dir(paths, seen, user_home.join(".codex"));
            discover_home_named_codex_dirs(&user_home, paths, seen);
        }
    }
    push_if_codex_dir(paths, seen, distro_path.join("root").join(".codex"));
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_wsl_distro_names(output: &[u8]) -> Vec<String> {
    let nul_odd_bytes = output
        .iter()
        .enumerate()
        .filter(|(index, byte)| index % 2 == 1 && **byte == 0)
        .count();
    let looks_utf16le = output.len() > 2 && nul_odd_bytes > output.len() / 4;

    let text = if looks_utf16le {
        let units: Vec<u16> = output
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(output).into_owned()
    };

    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for raw in text.lines() {
        let cleaned: String = raw.chars().filter(|ch| *ch != '\0').collect();
        let name = cleaned.trim();
        if !name.is_empty() && seen.insert(name.to_string()) {
            names.push(name.to_string());
        }
    }
    names
}

#[cfg(target_os = "windows")]
fn wsl_distro_names() -> Vec<String> {
    let mut command = std::process::Command::new("wsl.exe");
    command.args(["--list", "--quiet"]);

    crate::child_process::output_no_window(&mut command)
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_wsl_distro_names(&output.stdout))
        .unwrap_or_default()
}

#[cfg(target_os = "windows")]
fn discover_windows_wsl_codex_dirs(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    for namespace in ["\\\\wsl.localhost\\", "\\\\wsl$\\"] {
        let root = PathBuf::from(namespace);
        let Ok(distros) = fs::read_dir(&root) else {
            continue;
        };

        for distro in distros.flatten() {
            discover_wsl_distro_codex_dirs(distro.path(), paths, seen);
        }
    }

    for distro in wsl_distro_names() {
        for namespace in ["\\\\wsl.localhost\\", "\\\\wsl$\\"] {
            discover_wsl_distro_codex_dirs(
                PathBuf::from(format!("{namespace}{distro}")),
                paths,
                seen,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn discover_windows_wsl_codex_dirs(_paths: &mut Vec<PathBuf>, _seen: &mut HashSet<PathBuf>) {}

pub fn discover_additional_codex_dirs(home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    discover_home_named_codex_dirs(home, &mut paths, &mut seen);
    discover_env_codex_dirs(home, &mut paths, &mut seen);
    discover_windows_wsl_codex_dirs(&mut paths, &mut seen);
    paths.sort_by_key(|p| p.to_string_lossy().to_string());
    paths
}

pub fn runtime_codex_dirs(config_dirs: &[String], home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for dir in config_dirs {
        if let Some(expanded) = expand_home_path(dir, home) {
            push_unique(&mut paths, &mut seen, expanded);
        }
    }

    push_unique(&mut paths, &mut seen, home.join(".codex"));

    for dir in discover_additional_codex_dirs(home) {
        push_unique(&mut paths, &mut seen, dir);
    }

    paths
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn looks_like_windows_wsl_path_string(raw: &str) -> bool {
    let normalized = raw.replace('/', "\\").to_ascii_lowercase();
    let stripped = normalized
        .strip_prefix("\\\\?\\unc\\")
        .unwrap_or(&normalized);

    stripped.starts_with("\\\\wsl.localhost\\")
        || stripped.starts_with("\\\\wsl$\\")
        || stripped.starts_with("wsl.localhost\\")
        || stripped.starts_with("wsl$\\")
}

#[cfg(target_os = "windows")]
fn is_allowed_outside_home(path: &Path) -> bool {
    looks_like_windows_wsl_path_string(&path.to_string_lossy())
}

#[cfg(not(target_os = "windows"))]
fn is_allowed_outside_home(_path: &Path) -> bool {
    false
}

pub fn validate_codex_dir(path: &str, home: &Path) -> bool {
    let Some(expanded) = expand_home_path(path, home) else {
        return false;
    };
    let Ok(canonical) = expanded.canonicalize() else {
        return false;
    };
    let home_canonical = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    if !canonical.starts_with(&home_canonical) && !is_allowed_outside_home(&canonical) {
        return false;
    }
    has_codex_sessions(&canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempHome {
        path: PathBuf,
    }

    impl TempHome {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "ai-token-monitor-codex-paths-{}-{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn expands_home_paths_with_unix_or_windows_separator() {
        let home = PathBuf::from("/tmp/home");

        assert_eq!(expand_home_path("~", &home).unwrap(), home);
        assert_eq!(
            expand_home_path("~/.codex", &home).unwrap(),
            PathBuf::from("/tmp/home/.codex")
        );
        assert_eq!(
            expand_home_path("~\\.codex", &home).unwrap(),
            PathBuf::from("/tmp/home/.codex")
        );
    }

    #[test]
    fn detects_named_codex_dirs_under_home() {
        let home = TempHome::new();
        fs::create_dir_all(home.path.join(".codex-work").join("sessions")).unwrap();
        fs::create_dir_all(home.path.join(".codex-empty")).unwrap();

        let found = discover_additional_codex_dirs(&home.path);

        assert!(found.contains(&home.path.join(".codex-work")));
        assert!(!found.contains(&home.path.join(".codex-empty")));
    }

    #[test]
    fn runtime_dirs_include_configured_primary_and_discovered_dirs_once() {
        let home = TempHome::new();
        fs::create_dir_all(home.path.join(".codex-work").join("sessions")).unwrap();
        let configured = vec![
            "~/.codex".to_string(),
            "~/.codex-work".to_string(),
            "~/.codex-work".to_string(),
        ];

        let dirs = runtime_codex_dirs(&configured, &home.path);

        assert_eq!(dirs.first(), Some(&home.path.join(".codex")));
        assert_eq!(
            dirs.iter()
                .filter(|path| **path == home.path.join(".codex-work"))
                .count(),
            1
        );
    }

    #[test]
    fn validate_codex_dir_accepts_home_config_and_rejects_outside_home() {
        let home = TempHome::new();
        fs::create_dir_all(home.path.join(".codex").join("sessions")).unwrap();
        let outside = TempHome::new();
        fs::create_dir_all(outside.path.join(".codex").join("sessions")).unwrap();

        assert!(validate_codex_dir("~/.codex", &home.path));
        assert!(!validate_codex_dir(
            &outside.path.join(".codex").display().to_string(),
            &home.path
        ));
    }

    #[test]
    fn recognizes_windows_wsl_unc_path_strings() {
        assert!(looks_like_windows_wsl_path_string(
            "\\\\wsl.localhost\\Ubuntu\\home\\xs\\.codex"
        ));
        assert!(looks_like_windows_wsl_path_string(
            "\\\\?\\UNC\\wsl$\\Ubuntu\\home\\xs\\.codex"
        ));
        assert!(!looks_like_windows_wsl_path_string(
            "\\\\server\\share\\home\\xs\\.codex"
        ));
    }

    #[test]
    fn parses_wsl_distro_names_from_utf8_or_utf16le_output() {
        assert_eq!(
            parse_wsl_distro_names(b"Ubuntu-24.04\r\ndocker-desktop\r\n"),
            vec!["Ubuntu-24.04", "docker-desktop"]
        );

        let utf16le: Vec<u8> = "Ubuntu-24.04\r\n\0docker-desktop\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();

        assert_eq!(
            parse_wsl_distro_names(&utf16le),
            vec!["Ubuntu-24.04", "docker-desktop"]
        );
    }
}
