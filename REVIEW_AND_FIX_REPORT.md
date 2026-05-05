# Review and Fix Report

## Changes
- Hardened `save_png_to_file` to require `.png` paths, validate the PNG signature, cap input at 50 MiB, and reject existing files instead of silently overwriting them.
- Removed the brittle compact-JSON prefilter from Claude JSONL parsing; parsing now checks `type == "assistant"` structurally after JSON decode.
- Replaced the renderer-facing decrypted `get_ai_keys` command with boolean key status plus field-level secret updates.
- Restricted the encrypted AI key file to user-only permissions on Unix platforms.
- Added regression tests for rejected PNG writes, assistant JSON lines containing normal whitespace, and secret-status/key-field storage behavior.

## Verification
- `cargo test --manifest-path src-tauri/Cargo.toml save_png_to_file --lib` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml parse_session_line --lib` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` passed.
- `npm run build` passed.
- `git diff --check` passed.

## Remaining
- The encrypted-file fallback still derives its key from local machine identity rather than an OS keychain. Renderer exposure is closed, but native keychain integration would be a separate storage migration.
