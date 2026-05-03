# Codex Local Environment

- `setup.sh` installs project dependencies, refreshes the Gigabrain Codex wiring, and runs a health check.
- `actions/install-gigabrain-mcp.sh` installs the Gigabrain MCP server into Codex on this machine.
- `actions/verify-gigabrain.sh` runs the Codex-aware Gigabrain doctor against both the repo store and the personal user store.
- `actions/run-gigabrain-maintenance.sh` runs a manual Gigabrain maintenance cycle for this repo.
- `actions/checkpoint-gigabrain-session.sh` writes a native-only Codex App session checkpoint into today’s Gigabrain daily log.
- This repo is wired to the shared standalone Gigabrain store at `~/.gigabrain`.
- Use `target: "user"` for stable personal preferences/facts in `~/.gigabrain/profile`; use `target: "project"` for repo-specific memory with scope `project:ai-token-monitor:a58fb715`.
- Prefer Gigabrain through MCP in Codex once `actions/install-gigabrain-mcp.sh` has been run.
- If MCP is unavailable, use these generated helper scripts or `npx --yes --package @legendaryvibecoder/gigabrain@<version> ...`. Do not hardcode `~/.npm/_npx/.../scripts/gigabrainctl.js` paths.
