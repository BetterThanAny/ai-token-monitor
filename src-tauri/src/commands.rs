use std::fs;
use std::path::PathBuf;

use crate::providers::claude_code::ClaudeCodeProvider;
use crate::providers::traits::TokenProvider;
use crate::providers::types::{AllStats, UserPreferences};

fn prefs_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-prefs.json")
}

#[tauri::command]
pub fn get_all_stats() -> Result<AllStats, String> {
    let provider = ClaudeCodeProvider::new();
    if !provider.is_available() {
        return Err("Claude Code stats not available".to_string());
    }
    provider.fetch_stats()
}

#[tauri::command]
pub fn get_preferences() -> UserPreferences {
    let path = prefs_path();
    if let Ok(content) = fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        UserPreferences::default()
    }
}

#[tauri::command]
pub fn set_preferences(app: tauri::AppHandle, prefs: UserPreferences) -> Result<(), String> {
    let path = prefs_path();
    let json = serde_json::to_string_pretty(&prefs)
        .map_err(|e| format!("Failed to serialize preferences: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write preferences: {}", e))?;
    crate::update_tray_title(&app);
    Ok(())
}
