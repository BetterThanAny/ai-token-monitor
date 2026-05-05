<!-- GIGABRAIN_CODEX_MEMORY_START -->
## Gigabrain Memory

- Repo memory uses the shared Gigabrain store at `~/.gigabrain`.
- Personal memory uses the durable user store at `~/.gigabrain/profile`.
- Use `gigabrain_recall` first for continuity, people, project decisions, and prior context in this workspace. Repo-specific continuity here should normally use `target: "project"` with `scope: "project:ai-token-monitor:a58fb715"`.
- Use `gigabrain_provenance` when the user asks where a memory came from or wants exact grounding.
- Use `gigabrain_remember` with `target: "user"` for stable personal preferences/facts and with `target: "project"` for repo-specific decisions, conventions, and context.
- Use `gigabrain_checkpoint` at task end for substantial completed work, especially after implementation, debugging, planning/compaction summaries, or before closing a session with decisions or open loops. In this workspace, checkpoints should usually use `scope: "project:ai-token-monitor:a58fb715"`.
- Prefer Gigabrain MCP tools over direct CLI writes whenever the MCP server is available.
- If MCP is unavailable, use the generated `.codex/actions/` helper scripts or `npx --yes --package @legendaryvibecoder/gigabrain@<version> ...`, not raw `node ~/.npm/_npx/.../scripts/gigabrainctl.js` cache paths.
- Do not grep Gigabrain store files directly unless the Gigabrain MCP server is unavailable.
- Prefer Gigabrain primary memory first, then any labeled remote bridge results.

<!-- GIGABRAIN_CODEX_MEMORY_END -->

## Project Review Rules

- Codex Fast-mode multipliers in `src-tauri/src/providers/codex.rs` are intentional
  project policy. The app records this as a credit-equivalent estimate in the
  existing `cost_usd` field; do not report it as an API/USD semantic bug unless
  the project owner explicitly changes the policy.
- `gpt-5.3-codex-spark` being priced at zero in `src-tauri/pricing.json` is
  intentional project policy while that Codex rate is treated as
  research-preview/not-final. Do not report the zero price as a bug unless the
  project owner explicitly changes the policy.
