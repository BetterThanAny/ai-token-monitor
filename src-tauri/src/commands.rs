use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::providers::claude_code::ClaudeCodeProvider;
use crate::providers::codex::CodexProvider;
use crate::providers::pricing;
use crate::providers::traits::TokenProvider;
use crate::providers::types::{
    AccountState, AiKeyStatus, AiKeys, AllStats, BalanceInfo, LimitWindowStatus, UserPreferences,
};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_PNG_EXPORT_BYTES: usize = 50 * 1024 * 1024;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::Manager;

#[cfg(test)]
static TEST_HOME_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
#[cfg(test)]
static TEST_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn app_home_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Ok(guard) = TEST_HOME_DIR.lock() {
            if let Some(path) = guard.clone() {
                return path;
            }
        }
    }

    dirs::home_dir().unwrap_or_default()
}

fn expand_user_config_path(path: &str, home: &Path) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        home.to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(path)
    }
}

fn display_config_path(path: &Path, home: &Path) -> String {
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

pub(crate) fn prefs_path() -> PathBuf {
    app_home_dir()
        .join(".claude")
        .join("ai-token-monitor-prefs.json")
}

#[tauri::command]
pub async fn get_all_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let prefs = get_preferences();
        let provider = ClaudeCodeProvider::new(prefs.config_dirs);
        if !provider.is_available() {
            return Err("Claude Code stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    if result.is_ok() {
        crate::update_tray_title(&app);
    }
    result
}

#[tauri::command]
pub async fn get_codex_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let prefs = get_preferences();
        let provider = CodexProvider::new(prefs.codex_dirs);
        if !provider.is_available() {
            return Err("Codex stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    if result.is_ok() {
        crate::update_tray_title(&app);
    }
    result
}

#[tauri::command]
pub async fn get_account_states(
    include_claude: Option<bool>,
    include_codex: Option<bool>,
    codex_dirs: Option<Vec<String>>,
) -> Result<Vec<AccountState>, String> {
    let prefs = get_preferences();
    let include_claude = include_claude.unwrap_or(prefs.include_claude);
    let include_codex = include_codex.unwrap_or(prefs.include_codex);
    let codex_dirs = codex_dirs.unwrap_or_else(|| prefs.codex_dirs.clone());
    let mut states = Vec::new();

    if include_claude {
        match crate::claude_usage::get_statusline_rate_limits_usage(&prefs.config_dirs) {
            Ok(Some(usage)) => {
                states.push(claude_quota_to_account_state_with_source(
                    usage,
                    "claude_statusline_rate_limits",
                ));
            }
            Err(error) => {
                states.push(empty_claude_account_state(error));
            }
            Ok(None) => {
                states.push(empty_claude_account_state(
                    crate::claude_usage::statusline_rate_limits_missing_message(&prefs.config_dirs),
                ));
            }
        }
    }

    if include_codex {
        let state = tauri::async_runtime::spawn_blocking(move || {
            let provider = CodexProvider::new(codex_dirs);
            if !provider.is_available() {
                return Ok(None);
            }
            provider.fetch_account_state()
        })
        .await
        .map_err(|e| e.to_string())??;

        if let Some(state) = state {
            states.push(state);
        }
    }

    Ok(states)
}

fn claude_quota_to_account_state_with_source(
    usage: crate::claude_usage::ClaudeQuotaUsage,
    source: &str,
) -> AccountState {
    let mut limit_windows = Vec::new();

    if let Some(window) = usage.five_hour {
        limit_windows.push(claude_limit_window("Claude 5h", window, source));
    }
    if let Some(window) = usage.seven_day {
        limit_windows.push(claude_limit_window("Claude 7d", window, source));
    }
    if let Some(window) = usage.seven_day_sonnet {
        limit_windows.push(claude_limit_window("Claude Sonnet 7d", window, source));
    }
    if let Some(window) = usage.seven_day_opus {
        limit_windows.push(claude_limit_window("Claude Opus 7d", window, source));
    }

    let balance = usage.extra_usage.and_then(|extra| {
        if !extra.is_enabled {
            return None;
        }
        let remaining = (extra.monthly_limit - extra.used_credits).max(0.0);
        Some(BalanceInfo {
            balance: Some(remaining),
            used: Some(extra.used_credits),
            total: Some(extra.monthly_limit),
            remaining: Some(remaining),
            unit: "usd".to_string(),
            currency: Some("USD".to_string()),
            expires_at: None,
            is_unlimited: false,
            status: usage_status(extra.utilization),
        })
    });

    AccountState {
        provider: "claude".to_string(),
        fetched_at: Some(usage.fetched_at),
        is_stale: usage.is_stale,
        limit_windows,
        rate_limits: Vec::new(),
        balance,
        client_distribution: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn empty_claude_account_state(diagnostic: String) -> AccountState {
    AccountState {
        provider: "claude".to_string(),
        fetched_at: None,
        is_stale: false,
        limit_windows: Vec::new(),
        rate_limits: Vec::new(),
        balance: None,
        client_distribution: Vec::new(),
        diagnostics: vec![diagnostic],
    }
}

fn claude_limit_window(
    name: &str,
    window: crate::claude_usage::UsageWindow,
    source: &str,
) -> LimitWindowStatus {
    LimitWindowStatus {
        name: name.to_string(),
        used_percent: Some(window.utilization),
        used: None,
        total: None,
        remaining: None,
        unit: "percent".to_string(),
        window_minutes: None,
        starts_at: None,
        ends_at: None,
        resets_at: window.resets_at,
        status: usage_status(window.utilization),
        source: source.to_string(),
    }
}

fn usage_status(utilization: f64) -> String {
    if utilization >= 100.0 {
        "exhausted".to_string()
    } else if utilization >= 90.0 {
        "critical".to_string()
    } else if utilization >= 70.0 {
        "warning".to_string()
    } else {
        "ok".to_string()
    }
}

#[tauri::command]
pub fn is_codex_available() -> bool {
    let prefs = get_preferences();
    CodexProvider::new(prefs.codex_dirs).is_available()
}

#[tauri::command]
pub fn detect_claude_dirs() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut found: Vec<String> = Vec::new();

    // Scan ~/.claude-* directories
    if let Ok(entries) = std::fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".claude-") && entry.path().join("projects").is_dir() {
                found.push(format!("~/{}", name));
            }
        }
    }

    // Check CLAUDE_CONFIG_DIR env var
    if let Ok(env_dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        let path = PathBuf::from(&env_dir);
        if path.join("projects").is_dir() {
            let display = display_config_path(&path, &home);
            if !found.contains(&display) && display != "~/.claude" {
                found.push(display);
            }
        }
    }

    found.sort();
    found
}

#[tauri::command]
pub fn detect_codex_dirs() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut found: Vec<String> = Vec::new();

    // Scan ~/.codex-* directories
    if let Ok(entries) = std::fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".codex-")
                && (entry.path().join("sessions").is_dir()
                    || entry.path().join("archived_sessions").is_dir())
            {
                found.push(format!("~/{}", name));
            }
        }
    }

    // Check CODEX_CONFIG_DIR env var
    if let Ok(env_dir) = std::env::var("CODEX_CONFIG_DIR") {
        let path = PathBuf::from(&env_dir);
        if path.join("sessions").is_dir() || path.join("archived_sessions").is_dir() {
            let display = display_config_path(&path, &home);
            if !found.contains(&display) && display != "~/.codex" {
                found.push(display);
            }
        }
    }

    found.sort();
    found
}

#[tauri::command]
pub fn validate_codex_dir(path: String) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let expanded = expand_user_config_path(&path, &home);
    // Guard against path traversal outside home directory
    let canonical = match expanded.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !canonical.starts_with(&home) {
        return false;
    }
    canonical.join("sessions").is_dir() || canonical.join("archived_sessions").is_dir()
}

#[tauri::command]
pub fn validate_claude_dir(path: String) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let expanded = expand_user_config_path(&path, &home);
    // Guard against path traversal outside home directory
    let canonical = match expanded.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !canonical.starts_with(&home) {
        return false;
    }
    canonical.join("projects").is_dir()
}

const APP_SALT: &[u8] = b"ai-token-monitor-v1";

/// Cached AI keys to avoid repeated file reads.
static AI_KEYS_CACHE: std::sync::Mutex<Option<Option<AiKeys>>> = std::sync::Mutex::new(None);

fn encrypted_keys_path() -> PathBuf {
    app_home_dir()
        .join(".claude")
        .join(".ai-token-monitor-keys.enc")
}

#[cfg(unix)]
fn restrict_secret_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|e| format!("Failed to inspect AI keys permissions: {}", e))?
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|e| format!("Failed to secure AI keys permissions: {}", e))
}

#[cfg(not(unix))]
fn restrict_secret_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn get_machine_id() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("IOPlatformUUID") {
                    if let Some(start) = line.find('"') {
                        let rest = &line[start + 1..];
                        if let Some(mid) = rest.find("\" = \"") {
                            let uuid_start = mid + 5;
                            if let Some(end) = rest[uuid_start..].find('"') {
                                return rest[uuid_start..uuid_start + end].to_string();
                            }
                        }
                    }
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("reg")
            .args([
                "query",
                r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Cryptography",
                "/v",
                "MachineGuid",
            ])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("MachineGuid") {
                    if let Some(guid) = line.split_whitespace().last() {
                        return guid.to_string();
                    }
                }
            }
        }
    }
    // Fallback: hostname + username
    let hostname = whoami::fallible::hostname().unwrap_or_else(|_| "unknown-host".to_string());
    format!("{}-{}", hostname, whoami::username())
}

fn derive_encryption_key() -> [u8; 32] {
    let machine_id = get_machine_id();
    let mut hasher = Sha256::new();
    hasher.update(machine_id.as_bytes());
    hasher.update(APP_SALT);
    hasher.finalize().into()
}

fn encrypt_data(plaintext: &[u8]) -> Option<String> {
    let key = derive_encryption_key();
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;
    let mut nonce_bytes = [0u8; 12];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext).ok()?;
    // Format: base64(nonce + ciphertext)
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Some(base64::engine::general_purpose::STANDARD.encode(&combined))
}

fn decrypt_data(encoded: &str) -> Option<Vec<u8>> {
    let key = derive_encryption_key();
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    if combined.len() < 12 {
        return None;
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).ok()
}

fn load_ai_keys() -> Option<AiKeys> {
    // Return cached value if available
    if let Ok(cache) = AI_KEYS_CACHE.lock() {
        if let Some(ref cached) = *cache {
            return cached.clone();
        }
    }

    let result = load_ai_keys_from_file();

    // Cache the result
    if let Ok(mut cache) = AI_KEYS_CACHE.lock() {
        *cache = Some(result.clone());
    }

    result
}

fn load_ai_keys_from_file() -> Option<AiKeys> {
    let path = encrypted_keys_path();
    let encoded = fs::read_to_string(&path).ok()?;
    let decrypted = decrypt_data(encoded.trim())?;
    let json_str = String::from_utf8(decrypted).ok()?;
    let keys: AiKeys = serde_json::from_str(&json_str).ok()?;
    if keys.has_any_key() {
        Some(keys)
    } else {
        None
    }
}

fn write_encrypted_keys(path: &Path, encrypted: &str) -> Result<(), String> {
    fs::write(path, encrypted).map_err(|e| format!("Failed to write AI keys: {}", e))?;
    restrict_secret_file_permissions(path)
}

fn save_ai_keys(keys: &Option<AiKeys>) -> Result<(), String> {
    let path = encrypted_keys_path();
    match keys {
        Some(k) if k.has_any_key() => {
            let json = serde_json::to_string(k)
                .map_err(|e| format!("Failed to serialize AI keys: {}", e))?;
            let encrypted = encrypt_data(json.as_bytes())
                .ok_or_else(|| "Failed to encrypt AI keys".to_string())?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create AI keys directory: {}", e))?;
            }
            write_encrypted_keys(&path, &encrypted)?;
        }
        _ => {
            // No keys — remove file
            match fs::remove_file(&path) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Failed to remove AI keys: {}", e)),
            }
        }
    }
    // Invalidate cache so next load picks up new values
    if let Ok(mut cache) = AI_KEYS_CACHE.lock() {
        *cache = None;
    }
    Ok(())
}

fn ai_key_status() -> AiKeyStatus {
    load_ai_keys()
        .as_ref()
        .map(AiKeys::status)
        .unwrap_or_default()
}

fn normalized_secret(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn set_ai_key_field_value(
    keys: &mut AiKeys,
    field: &str,
    value: Option<String>,
) -> Result<(), String> {
    match field {
        "gemini" => keys.gemini = value,
        "openai" => keys.openai = value,
        "anthropic" => keys.anthropic = value,
        "webhook_discord_url" => keys.webhook_discord_url = value,
        "webhook_slack_url" => keys.webhook_slack_url = value,
        "webhook_telegram_bot_token" => keys.webhook_telegram_bot_token = value,
        "webhook_telegram_chat_id" => keys.webhook_telegram_chat_id = value,
        _ => return Err(format!("Unknown AI key field: {}", field)),
    }
    Ok(())
}

#[tauri::command]
pub fn get_preferences() -> UserPreferences {
    let path = prefs_path();
    let mut prefs: UserPreferences = if let Ok(content) = fs::read_to_string(&path) {
        match serde_json::from_str(&content) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[PREFS] Failed to parse prefs: {e}. Backing up and using defaults.");
                let backup = path.with_extension("json.bak");
                let _ = fs::copy(&path, &backup);
                UserPreferences::default()
            }
        }
    } else {
        UserPreferences::default()
    };

    let mut prefs_changed = false;

    // Migrate: if ai_keys exist in JSON file, move them to encrypted file
    if let Some(legacy_keys) = prefs.ai_keys.take() {
        match save_ai_keys(&Some(legacy_keys)) {
            Ok(()) => {
                prefs_changed = true;
            }
            Err(e) => {
                eprintln!("[PREFS] Failed to migrate AI keys to encrypted storage: {e}");
            }
        }
    }

    if prefs_changed {
        if let Ok(json) = serde_json::to_string_pretty(&prefs) {
            let _ = fs::write(&path, &json);
        }
    }

    // ai_keys are intentionally not returned to the renderer.
    prefs
}

/// Load AI keys from encrypted local storage for backend-only webhook work.
pub(crate) fn get_ai_keys() -> Option<AiKeys> {
    load_ai_keys()
}

#[tauri::command]
pub fn get_ai_key_status() -> AiKeyStatus {
    ai_key_status()
}

#[tauri::command]
pub fn set_ai_key_field(field: String, value: Option<String>) -> Result<AiKeyStatus, String> {
    let mut keys = load_ai_keys().unwrap_or_default();
    set_ai_key_field_value(&mut keys, &field, normalized_secret(value))?;
    let next_keys = if keys.has_any_key() { Some(keys) } else { None };
    save_ai_keys(&next_keys)?;
    Ok(next_keys.as_ref().map(AiKeys::status).unwrap_or_default())
}

fn persist_preferences(prefs: &UserPreferences) -> Result<(), String> {
    let mut file_prefs = prefs.clone();
    file_prefs.ai_keys = None; // Never write keys to preferences JSON

    let path = prefs_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create preferences directory: {}", e))?;
    }
    let json = serde_json::to_string_pretty(&file_prefs)
        .map_err(|e| format!("Failed to serialize preferences: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write preferences: {}", e))
}

#[tauri::command]
pub fn set_preferences(app: tauri::AppHandle, prefs: UserPreferences) -> Result<(), String> {
    persist_preferences(&prefs)?;
    crate::update_tray_title(&app);
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(deprecated)]
pub fn copy_png_to_clipboard(png_data: Vec<u8>) -> Result<(), String> {
    #[allow(deprecated)]
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let ns_data: id =
            msg_send![class!(NSData), dataWithBytes:png_data.as_ptr() length:png_data.len()];
        if ns_data == nil {
            return Err("Failed to create NSData".to_string());
        }

        let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
        let _: () = msg_send![pasteboard, clearContents];
        let png_type: id =
            msg_send![class!(NSString), stringWithUTF8String: c"public.png".as_ptr()];
        let success: bool = msg_send![pasteboard, setData: ns_data forType: png_type];

        if success {
            Ok(())
        } else {
            Err("Failed to copy to clipboard".to_string())
        }
    }
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn copy_png_to_clipboard(png_data: Vec<u8>) -> Result<(), String> {
    // On Windows, decode PNG to bitmap and use CF_DIB
    // For simplicity, write PNG to temp file and use GDI+
    // Fallback: just return error, user can use native capture
    Err("Image clipboard not yet supported on Windows — use screenshot instead".to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
pub fn copy_png_to_clipboard(_png_data: Vec<u8>) -> Result<(), String> {
    Err("Image clipboard not supported on this platform".to_string())
}

#[tauri::command]
pub fn save_png_to_file(png_data: Vec<u8>, path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    let parent = path.parent().ok_or("Invalid path")?;
    let file_name = path.file_name().ok_or("Invalid path")?;

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or("Destination file must use .png extension")?;
    if !extension.eq_ignore_ascii_case("png") {
        return Err("Destination file must use .png extension".to_string());
    }

    if png_data.len() > MAX_PNG_EXPORT_BYTES {
        return Err(format!(
            "PNG exceeds maximum size of {} bytes",
            MAX_PNG_EXPORT_BYTES
        ));
    }
    if !png_data.starts_with(PNG_SIGNATURE) {
        return Err("Invalid PNG data".to_string());
    }

    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| "Invalid destination directory".to_string())?;
    let home = app_home_dir()
        .canonicalize()
        .map_err(|_| "Cannot determine home directory".to_string())?;
    if !canonical_parent.starts_with(&home) {
        return Err("Destination must be within home directory".to_string());
    }
    let safe_path = canonical_parent.join(file_name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&safe_path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                "Destination file already exists".to_string()
            } else {
                format!("Failed to save PNG: {}", e)
            }
        })?;
    std::io::Write::write_all(&mut file, &png_data)
        .map_err(|e| format!("Failed to save PNG: {}", e))
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(deprecated)]
pub fn capture_window(app: tauri::AppHandle) -> Result<(), String> {
    #[allow(deprecated)]
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};

    let window = app.get_webview_window("main").ok_or("Window not found")?;

    // Get the native NSWindow number
    let ns_window: id = window
        .ns_window()
        .map_err(|e| format!("Failed to get NSWindow: {}", e))? as id;
    let window_number: i64 = unsafe { msg_send![ns_window, windowNumber] };

    unsafe {
        // CGWindowListCreateImage with the specific window
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGWindowListCreateImage(
                screenBounds: CGRect,
                listOption: u32,
                windowID: u32,
                imageOption: u32,
            ) -> id;
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFRelease(cf: id);
        }

        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGPoint {
            x: f64,
            y: f64,
        }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGSize {
            width: f64,
            height: f64,
        }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGRect {
            origin: CGPoint,
            size: CGSize,
        }

        let null_rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: 0.0,
                height: 0.0,
            },
        };

        // kCGWindowListOptionIncludingWindow = 1 << 3 = 8
        // kCGWindowImageBoundsIgnoreFraming = 1 << 0 = 1
        let cg_image = CGWindowListCreateImage(null_rect, 8, window_number as u32, 1);
        if cg_image == nil {
            return Err("Failed to capture window".to_string());
        }

        // Convert CGImage to PNG NSData via NSBitmapImageRep
        let ns_bitmap_rep: id = msg_send![class!(NSBitmapImageRep), alloc];
        let ns_bitmap_rep: id = msg_send![ns_bitmap_rep, initWithCGImage: cg_image];
        if ns_bitmap_rep == nil {
            CFRelease(cg_image);
            return Err("Failed to create bitmap rep".to_string());
        }

        // representationUsingType:NSPNGFileType properties:nil
        // NSPNGFileType = 4 (NSBitmapImageFileType)
        let png_data: id = msg_send![
            ns_bitmap_rep,
            representationUsingType: 4u64
            properties: nil
        ];
        if png_data == nil {
            let _: () = msg_send![ns_bitmap_rep, release];
            CFRelease(cg_image);
            return Err("Failed to create PNG data".to_string());
        }

        // Copy to pasteboard
        let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
        let _: () = msg_send![pasteboard, clearContents];
        let png_type: id =
            msg_send![class!(NSString), stringWithUTF8String: c"public.png".as_ptr()];
        let success: bool = msg_send![pasteboard, setData: png_data forType: png_type];

        // Cleanup
        let _: () = msg_send![ns_bitmap_rep, release];
        CFRelease(cg_image);

        if success {
            Ok(())
        } else {
            Err("Failed to copy to clipboard".to_string())
        }
    }
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn capture_window(app: tauri::AppHandle) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetWindowDC, ReleaseDC, SelectObject, SRCCOPY,
    };
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };

    let window = app.get_webview_window("main").ok_or("Window not found")?;

    let hwnd = window
        .hwnd()
        .map_err(|e| format!("Failed to get HWND: {}", e))?;
    let hwnd = HWND(hwnd.0);

    unsafe {
        // Get window dimensions via GetWindowDC + bitmap size
        let hdc_window = GetWindowDC(Some(hwnd));
        let mut rect = windows::Win32::Foundation::RECT::default();
        windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect)
            .map_err(|e| format!("GetWindowRect: {}", e))?;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;

        let hdc_mem = CreateCompatibleDC(Some(hdc_window));
        let hbm = CreateCompatibleBitmap(hdc_window, width, height);
        let old_obj = SelectObject(hdc_mem, hbm.into());

        // Capture window content via BitBlt
        let _ = BitBlt(
            hdc_mem,
            0,
            0,
            width,
            height,
            Some(hdc_window),
            0,
            0,
            SRCCOPY,
        );

        // Deselect bitmap from DC before clipboard operations
        SelectObject(hdc_mem, old_obj);

        // Clean up GDI objects before clipboard
        DeleteDC(hdc_mem);
        ReleaseDC(Some(hwnd), hdc_window);

        // Copy to clipboard
        if OpenClipboard(Some(hwnd)).is_err() {
            DeleteObject(hbm.into());
            return Err("Failed to open clipboard".to_string());
        }
        let _ = EmptyClipboard();
        // CF_BITMAP = 2
        let result = SetClipboardData(2, Some(windows::Win32::Foundation::HANDLE(hbm.0)));
        let _ = CloseClipboard();
        match result {
            Ok(_) => Ok(()),
            Err(_) => {
                DeleteObject(hbm.into());
                Err("Failed to copy to clipboard".to_string())
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
pub fn capture_window(_app: tauri::AppHandle) -> Result<(), String> {
    Err("Screenshot not supported on this platform".to_string())
}

#[tauri::command]
pub fn get_pricing_table() -> pricing::PricingTable {
    pricing::get_pricing_table()
}

#[tauri::command]
pub async fn test_webhook(platform: String) -> Result<String, String> {
    let secrets = load_ai_keys().ok_or("No webhook credentials configured")?;
    crate::webhooks::test_webhook_endpoint(&platform, &secrets).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestHomeGuard {
        path: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl TestHomeGuard {
        fn new() -> Self {
            let lock = TEST_HOME_LOCK.lock().unwrap();
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ai-token-monitor-home-{}-{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(path.join(".claude")).unwrap();
            *TEST_HOME_DIR.lock().unwrap() = Some(path.clone());
            *AI_KEYS_CACHE.lock().unwrap() = None;
            Self { path, _lock: lock }
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            *AI_KEYS_CACHE.lock().unwrap() = None;
            *TEST_HOME_DIR.lock().unwrap() = None;
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn expands_home_config_paths() {
        let home = PathBuf::from("/tmp/ai-token-monitor-home");

        assert_eq!(expand_user_config_path("~", &home), home);
        assert_eq!(
            expand_user_config_path("~/.codex", &home),
            PathBuf::from("/tmp/ai-token-monitor-home/.codex")
        );
    }

    #[test]
    fn displays_home_relative_config_paths() {
        let home = PathBuf::from("/tmp/ai-token-monitor-home");

        assert_eq!(
            display_config_path(&PathBuf::from("/tmp/ai-token-monitor-home/.codex"), &home),
            "~/.codex"
        );
        assert_eq!(
            display_config_path(&PathBuf::from("/var/tmp/.codex"), &home),
            "/var/tmp/.codex"
        );
    }

    #[test]
    fn persisting_plain_preferences_preserves_existing_encrypted_ai_keys() {
        let _guard = TestHomeGuard::new();
        let keys = Some(AiKeys {
            webhook_discord_url: Some(
                "https://discord.com/api/webhooks/123456789/token".to_string(),
            ),
            ..AiKeys::default()
        });

        save_ai_keys(&keys).unwrap();
        assert!(encrypted_keys_path().exists());

        let prefs = UserPreferences {
            ai_keys: None,
            show_tray_cost: false,
            ..UserPreferences::default()
        };
        persist_preferences(&prefs).unwrap();

        assert_eq!(
            load_ai_keys()
                .and_then(|k| k.webhook_discord_url)
                .as_deref(),
            Some("https://discord.com/api/webhooks/123456789/token")
        );
        let stored_prefs = fs::read_to_string(prefs_path()).unwrap();
        assert!(!stored_prefs.contains("webhook_discord_url"));
    }

    #[test]
    fn ai_key_status_reports_presence_without_values() {
        let _guard = TestHomeGuard::new();
        let keys = Some(AiKeys {
            webhook_discord_url: Some(
                "https://discord.com/api/webhooks/123456789/token".to_string(),
            ),
            webhook_telegram_bot_token: Some("telegram-token".to_string()),
            ..AiKeys::default()
        });

        save_ai_keys(&keys).unwrap();

        let status = get_ai_key_status();
        assert!(status.webhook_discord_url);
        assert!(status.webhook_telegram_bot_token);
        assert!(!status.webhook_slack_url);
        assert!(!status.webhook_telegram_chat_id);
    }

    #[test]
    fn set_ai_key_field_preserves_unmodified_existing_keys() {
        let _guard = TestHomeGuard::new();
        let keys = Some(AiKeys {
            webhook_discord_url: Some(
                "https://discord.com/api/webhooks/123456789/token".to_string(),
            ),
            webhook_slack_url: Some("https://hooks.slack.com/services/T000/B000/old".to_string()),
            ..AiKeys::default()
        });

        save_ai_keys(&keys).unwrap();
        let status = set_ai_key_field(
            "webhook_slack_url".to_string(),
            Some(" https://hooks.slack.com/services/T000/B000/new ".to_string()),
        )
        .unwrap();

        assert!(status.webhook_discord_url);
        assert!(status.webhook_slack_url);
        let stored = load_ai_keys().unwrap();
        assert_eq!(
            stored.webhook_discord_url.as_deref(),
            Some("https://discord.com/api/webhooks/123456789/token")
        );
        assert_eq!(
            stored.webhook_slack_url.as_deref(),
            Some("https://hooks.slack.com/services/T000/B000/new")
        );
    }

    #[test]
    fn set_ai_key_field_clears_only_selected_key() {
        let _guard = TestHomeGuard::new();
        let keys = Some(AiKeys {
            webhook_discord_url: Some(
                "https://discord.com/api/webhooks/123456789/token".to_string(),
            ),
            webhook_slack_url: Some("https://hooks.slack.com/services/T000/B000/old".to_string()),
            ..AiKeys::default()
        });

        save_ai_keys(&keys).unwrap();
        let status = set_ai_key_field("webhook_slack_url".to_string(), None).unwrap();

        assert!(status.webhook_discord_url);
        assert!(!status.webhook_slack_url);
        let stored = load_ai_keys().unwrap();
        assert!(stored.webhook_discord_url.is_some());
        assert!(stored.webhook_slack_url.is_none());
    }

    #[test]
    fn set_ai_key_field_rejects_unknown_fields() {
        let _guard = TestHomeGuard::new();

        let err = set_ai_key_field("not_a_real_field".to_string(), Some("value".to_string()))
            .expect_err("unknown fields should be rejected");

        assert_eq!(err, "Unknown AI key field: not_a_real_field");
        assert!(load_ai_keys().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn encrypted_ai_keys_file_is_user_only() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = TestHomeGuard::new();
        let keys = Some(AiKeys {
            webhook_discord_url: Some(
                "https://discord.com/api/webhooks/123456789/token".to_string(),
            ),
            ..AiKeys::default()
        });

        save_ai_keys(&keys).unwrap();

        let mode = fs::metadata(encrypted_keys_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    fn minimal_png_bytes() -> Vec<u8> {
        PNG_SIGNATURE.to_vec()
    }

    #[test]
    fn save_png_to_file_rejects_non_png_data() {
        let guard = TestHomeGuard::new();
        let path = guard.path.join("not-png.png");

        let err = save_png_to_file(b"not a png".to_vec(), path.display().to_string())
            .expect_err("non-PNG data should be rejected");

        assert_eq!(err, "Invalid PNG data");
        assert!(!path.exists());
    }

    #[test]
    fn save_png_to_file_rejects_non_png_extension() {
        let guard = TestHomeGuard::new();
        let path = guard.path.join("image.txt");

        let err = save_png_to_file(minimal_png_bytes(), path.display().to_string())
            .expect_err("non-.png extension should be rejected");

        assert_eq!(err, "Destination file must use .png extension");
        assert!(!path.exists());
    }

    #[test]
    fn save_png_to_file_rejects_existing_file() {
        let guard = TestHomeGuard::new();
        let path = guard.path.join("existing.png");
        fs::write(&path, b"existing").unwrap();

        let err = save_png_to_file(minimal_png_bytes(), path.display().to_string())
            .expect_err("existing file should not be overwritten");

        assert_eq!(err, "Destination file already exists");
        assert_eq!(fs::read(&path).unwrap(), b"existing");
    }
}
