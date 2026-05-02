# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> **[한국어](docs/README.ko.md) | [日本語](docs/README.ja.md) | [简体中文](docs/README.zh-CN.md) | [繁體中文](docs/README.zh-TW.md) | [Türkçe](docs/README.tr.md) | [Italiano](docs/README.it.md)**

A system tray app for macOS and Windows that tracks **Claude Code** and **Codex** token usage, cost, and activity in real time — with optional webhook alerts.

<table>
  <tr>
    <th width="50%">Overview</th>
    <th width="50%">Analytics</th>
  </tr>
  <tr>
    <td><img src="docs/screenshots/overview.png" width="280" /></td>
    <td><img src="docs/screenshots/analytics.png" width="280" /></td>
  </tr>
  <tr>
    <td align="center">Today's usage, 7-day chart, weekly/monthly totals</td>
    <td align="center">Activity graph, 30-day trends, model breakdown</td>
  </tr>
</table>

## Download

**[Download Latest Release](https://github.com/soulduse/ai-token-monitor/releases/latest)**

| Platform | File | Notes |
|----------|------|-------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac support coming soon |
| **Windows** | `.exe` installer | Windows 10+ (WebView2 required, auto-installed) |

## Features

### Tracking & Visualization
- **Real-time token tracking** — parses session JSONL files from Claude Code and Codex for accurate usage stats
- **Multi-provider support** — switch between Claude / Codex sources, with per-provider cost models
- **Multiple config directories** — aggregate work + personal accounts by adding several Claude/Codex roots
- **Daily chart** — 7/30 day token or cost bar chart with Y-axis labels
- **Activity graph** — GitHub-style contribution heatmap with 2D/3D toggle and year navigation
- **Period navigation** — browse weekly/monthly totals with `< >` arrows
- **Model breakdown** — Input/Output/Cache ratio visualization
- **Cache efficiency** — donut chart showing cache hit ratio

### Social & Sharing
- **AI Report (Wrapped)** — monthly/yearly recap card with top model, busiest day, and streaks
- **Receipt view** — receipt-style usage summary for today / week / month / all-time
- **Salary comparator** — see your monthly AI spend as a share of your salary (lattes / Netflix / chicken)
- **Share & export** — copy summary markdown, capture screenshot, or copy an app share message from the header menu

### Notifications & Alerts
- **Tray cost** — today's cost shown next to the tray icon (macOS menu bar title, Windows tooltip)
- **Webhook notifications** — Discord, Slack, and Telegram alerts when Claude OAuth usage tracking is enabled and usage crosses thresholds or resets
- **Auto-updater** — in-app update notifications with download progress

### Customization
- **4 themes** — GitHub (green), Purple, Ocean, Sunset — with Auto/Light/Dark mode
- **10 languages** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **Compact / full number format** — `377.0K` vs `377,000`
- **Launch on startup** — optional auto-start on boot
- **Auto-hide window** — hides when clicking outside

## Install from Source

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) toolchain
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code) or [Codex](https://openai.com/index/introducing-codex/) installed and used at least once

### Build

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # development mode
npm run tauri build   # production build
```

## Usage

### Basics

1. Launch the app — an icon appears in the system tray (macOS menu bar / Windows taskbar)
2. Click the icon to open the dashboard
3. Switch between **Overview** and **Analytics** tabs

### Tabs

| Tab | Content |
|-----|---------|
| **Overview** | Today's summary, 7-day chart, weekly/monthly totals, 8-week heatmap |
| **Analytics** | Full-year activity graph (2D/3D), 30-day chart, model breakdown, cache efficiency |

### Settings

Settings is organized into three tabs:

| Tab | Options |
|-----|---------|
| **General** | Theme, language, appearance, number format, menu bar cost, launch on startup, monthly salary, optional Claude usage tracking |
| **Account** | Claude/Codex config directories |
| **Webhooks** | Discord / Slack / Telegram webhook URLs, alert thresholds, monitored windows, reset notifications |

## Data Sources

| Provider | Path | Notes |
|----------|------|-------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | Session/tool-call counts from `~/.claude/stats-cache.json`. Supports multiple roots. |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | Supports multiple roots. |

**Network requests**: core usage tracking runs locally. Network requests occur when webhook alerts are sent, the updater checks releases, or you open external links from the app.

## Architecture

```
┌────────────────────────────────────┐
│  Frontend (React 19 + Vite)        │
│  ├── PopoverShell / Header         │
│  ├── TabBar (2 tabs)               │
│  ├── TodaySummary / DailyChart     │
│  ├── ActivityGraph (2D/3D) / Heatmap│
│  ├── ModelBreakdown / CacheEfficiency│
│  ├── Wrapped / Receipt             │
│  ├── SalaryComparator                 │
│  └── SettingsOverlay (3 tabs)      │
├────────────────────────────────────┤
│  Backend (Tauri v2 / Rust)         │
│  ├── JSONL session parsers (Claude/Codex)│
│  ├── File watcher (notify)         │
│  ├── Tray icon + cost display      │
│  ├── Auto-updater                  │
│  ├── Webhook dispatcher            │
│  └── Preferences + encrypted secrets│
├────────────────────────────────────┤
│  External services (opt-in)        │
│  └── Discord / Slack / Telegram    │
└────────────────────────────────────┘
```

## Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| **macOS** | Supported | Menu bar integration, dock hiding, tray cost title |
| **Windows** | Supported | System tray integration, NSIS installer, tooltip cost display |
| **Linux** | Untested | May work since Tauri supports Linux |

## Support

If you find this project useful, consider [buying me a coffee](https://ctee.kr/place/programmingzombie).

## License

MIT
