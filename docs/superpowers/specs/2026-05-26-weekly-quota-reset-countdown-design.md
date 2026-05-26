# Weekly Quota Reset Countdown Design

## Goal

Add a compact countdown to Analytics > Limits so users can see how long remains before Claude and Codex weekly quota windows reset.

## Scope

In scope:

- Show one Claude weekly reset countdown and one Codex weekly reset countdown in the Limits view.
- Reuse existing `AccountState.limit_windows[].resets_at` data.
- Refresh countdown text once per minute while the Limits view is mounted.
- Add localized labels for the countdown summary and missing/resetting states.
- Add focused tests for weekly-window selection and countdown formatting.

Out of scope:

- No new external API calls.
- No changes to Claude or Codex token/cost parsing.
- No inferred reset time when the source data does not include a usable weekly reset timestamp.
- No display of Claude Sonnet 7d or Claude Opus 7d in the top weekly reset summary.

## Data Selection

The implementation stays in the existing account-state flow. It does not add backend fields.

Claude:

- Use the `provider === "claude"` account state.
- Select only the limit window whose `name` is exactly `Claude 7d`.
- Ignore `Claude Sonnet 7d` and `Claude Opus 7d` for the top summary.

Codex:

- Use the `provider === "codex"` account state.
- Select only a clear seven-day quota window.
- Prefer `window_minutes === 10080`.
- If no clear seven-day window exists, keep the Codex row and show the unavailable state.

Invalid, empty, or unparsable `resets_at` values are treated as unavailable. The app does not estimate a reset timestamp from the current time or usage percentage.

## UI

Add a compact weekly-reset summary inside the existing Limits > Quota Windows card, above the current list of window progress rows.

The summary has two rows:

- Claude: countdown or unavailable state.
- Codex: countdown or unavailable state.

The summary uses the existing card visual language and a light divider so it reads as part of Quota Windows rather than a separate card. Countdown values use slightly stronger weight, but not large hero-style numbers.

The existing per-window rows remain below the summary and keep their current absolute reset-time display.

## Countdown Behavior

Countdown format is minute-level:

- Example: `6天 03小时 12分` in Chinese.
- English and other locales use short localized units.
- The display updates every 60 seconds.
- The timer only changes local React state and does not force a backend account-state refresh.

If `resets_at <= now`, show the resetting state. The next backend refresh can replace the stale window with a new reset timestamp.

## Localization

Add localized keys for:

- `limits.weeklyReset`
- `limits.weeklyResetUnavailable`
- `limits.resetting`
- day, hour, and minute countdown units

All existing locale JSON files must remain complete.

## Tests And Validation

Add focused frontend tests for pure helper functions such as weekly-window selection and countdown formatting.

Coverage should include:

- Claude selects only `Claude 7d`.
- Codex selects only a seven-day window with `window_minutes === 10080`.
- Missing weekly windows show the unavailable state.
- Invalid timestamps show the unavailable state.
- Expired timestamps show the resetting state.
- Future timestamps format as day/hour/minute countdown text.

Run `npm run build` after implementation. If Rust files stay untouched, full Rust tests are not required for this change.

## Acceptance Criteria

- The Limits view shows a Claude and Codex weekly quota reset summary at the top of the Quota Windows card.
- The countdown refreshes once per minute without re-reading account-state data.
- Missing weekly reset timestamps are explicit rather than hidden.
- Existing quota window rows and account-state polling behavior keep working.
- Type checking and the production frontend build pass.
