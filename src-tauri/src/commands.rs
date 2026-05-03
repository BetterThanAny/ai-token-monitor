use std::fs;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::providers::claude_code::ClaudeCodeProvider;
use crate::providers::codex::CodexProvider;
use crate::providers::pricing;
use crate::providers::traits::TokenProvider;
use crate::providers::types::{
    AccountState, AiKeys, AllStats, BalanceInfo, LimitWindowStatus, UserPreferences,
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::Manager;

#[cfg(test)]
static TEST_HOME_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

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
pub async fn get_account_states() -> Result<Vec<AccountState>, String> {
    let prefs = get_preferences();
    let mut states = Vec::new();

    if prefs.include_claude {
        match crate::claude_usage::get_statusline_rate_limits_usage() {
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
                    "Claude quota snapshots are only read from local statusLine data. Configure Claude Code statusLine to write ~/.claude/ai-token-monitor-rate-limits.json; local transcript logs are still used for token and cost totals."
                        .to_string(),
                ));
            }
        }
    }

    if prefs.include_codex {
        let codex_dirs = prefs.codex_dirs.clone();
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
            let display = if let Ok(stripped) = path.strip_prefix(&home) {
                format!("~/{}", stripped.display())
            } else {
                env_dir
            };
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
            let display = if let Ok(stripped) = path.strip_prefix(&home) {
                format!("~/{}", stripped.display())
            } else {
                env_dir
            };
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
    let expanded = if path.starts_with("~/") {
        home.join(path.strip_prefix("~/").unwrap_or(&path))
    } else {
        PathBuf::from(&path)
    };
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
    let expanded = if path.starts_with("~/") {
        home.join(path.strip_prefix("~/").unwrap_or(&path))
    } else {
        PathBuf::from(&path)
    };
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
            fs::write(&path, &encrypted).map_err(|e| format!("Failed to write AI keys: {}", e))?;
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
    if prefs.ai_keys.is_some() {
        match save_ai_keys(&prefs.ai_keys) {
            Ok(()) => {
                prefs.ai_keys = None;
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

    // ai_keys are loaded separately via get_ai_keys command
    prefs
}

/// Load AI keys from encrypted local file on-demand.
#[tauri::command]
pub fn get_ai_keys() -> Option<AiKeys> {
    load_ai_keys()
}

#[tauri::command]
pub fn set_ai_keys(keys: Option<AiKeys>) -> Result<(), String> {
    save_ai_keys(&keys)
}

#[tauri::command]
pub fn clear_ai_keys() -> Result<(), String> {
    save_ai_keys(&None)
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
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| "Invalid destination directory".to_string())?;
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    if !canonical_parent.starts_with(&home) {
        return Err("Destination must be within home directory".to_string());
    }
    fs::write(&path, &png_data).map_err(|e| format!("Failed to save PNG: {}", e))
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
            // Release CGImage
            let _: () = msg_send![cg_image, release];
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
        // CGImage is a CF type, use CFRelease
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFRelease(cf: id);
        }
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
        // Do NOT delete hbm — clipboard owns it after SetClipboardData

        result
            .map(|_| ())
            .map_err(|_| "Failed to copy to clipboard".to_string())
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
    }

    impl TestHomeGuard {
        fn new() -> Self {
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
            Self { path }
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
}
