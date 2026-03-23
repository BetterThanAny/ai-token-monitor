use std::fs;
use std::path::PathBuf;

use crate::providers::claude_code::ClaudeCodeProvider;
use crate::providers::traits::TokenProvider;
use crate::providers::types::{AllStats, UserPreferences};

#[cfg(target_os = "macos")]
use tauri::Manager;

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
    // Update tray in background to avoid blocking the IPC response
    let handle = app.clone();
    std::thread::spawn(move || {
        crate::update_tray_title(&handle);
    });
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(deprecated)]
pub fn capture_window(app: tauri::AppHandle) -> Result<(), String> {
    #[allow(deprecated)]
    use cocoa::base::{id, nil};
    use objc::{msg_send, sel, sel_impl, class};

    let window = app.get_webview_window("main")
        .ok_or("Window not found")?;

    // Get the native NSWindow number
    let ns_window: id = window.ns_window()
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
        struct CGPoint { x: f64, y: f64 }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGSize { width: f64, height: f64 }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGRect { origin: CGPoint, size: CGSize }

        let null_rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize { width: 0.0, height: 0.0 },
        };

        // kCGWindowListOptionIncludingWindow = 1 << 3 = 8
        // kCGWindowImageBoundsIgnoreFraming = 1 << 0 = 1
        let cg_image = CGWindowListCreateImage(null_rect, 8, window_number as u32, 1);
        if cg_image == nil {
            return Err("Failed to capture window".to_string());
        }

        // Convert CGImage to PNG NSData via NSBitmapImageRep
        let ns_bitmap_rep: id = msg_send![
            class!(NSBitmapImageRep),
            alloc
        ];
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
        let png_type: id = msg_send![class!(NSString), stringWithUTF8String: b"public.png\0".as_ptr()];
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

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn capture_window(_app: tauri::AppHandle) -> Result<(), String> {
    Err("Screenshot not supported on this platform".to_string())
}
