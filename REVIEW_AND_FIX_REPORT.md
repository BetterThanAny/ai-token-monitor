# Review and Fix Report

## Changes
- Hardened `save_png_to_file` to require `.png` paths, validate the PNG signature, cap input at 50 MiB, and reject existing files instead of silently overwriting them.
- Removed the brittle compact-JSON prefilter from Claude JSONL parsing; parsing now checks `type == "assistant"` structurally after JSON decode.
- Added regression tests for rejected PNG writes and assistant JSON lines containing normal whitespace.

## Verification
- `cargo test --manifest-path src-tauri/Cargo.toml save_png_to_file --lib` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml parse_session_line --lib` passed.
- `git diff --check` passed.

## Remaining
- `get_ai_keys` still returns decrypted secrets to the renderer. I left it unchanged because fixing it safely requires a broader settings/API redesign.
