# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> **[한국어](docs/README.ko.md) | [日本語](docs/README.ja.md) | [简体中文](docs/README.zh-CN.md) | [繁體中文](docs/README.zh-TW.md)**

A system tray app for macOS and Windows that tracks Claude Code token usage and costs in real time.

<table>
  <tr>
    <th width="33%">Overview</th>
    <th width="33%">Analytics</th>
    <th width="33%">Leaderboard</th>
  </tr>
  <tr>
    <td><img src="docs/screenshots/overview.png" width="280" /></td>
    <td><img src="docs/screenshots/analytics.png" width="280" /></td>
    <td><img src="docs/screenshots/leaderboard.png" width="280" /></td>
  </tr>
  <tr>
    <td align="center">Today's usage, 7-day chart, weekly/monthly totals</td>
    <td align="center">Activity graph, 30-day trends, model breakdown</td>
    <td align="center">Compare usage with others</td>
  </tr>
</table>

## Download

**[Download Latest Release](https://github.com/soulduse/ai-token-monitor/releases/latest)**

| Platform | File | Notes |
|----------|------|-------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac support coming soon |
| **Windows** | `.exe` installer | Windows 10+ (WebView2 required, auto-installed) |

## Features

- **Real-time Token Tracking** — Parses Claude Code session JSONL files for accurate usage stats
- **Cost Calculation** — Automatic cost estimation based on model pricing (Opus, Sonnet, Haiku)
- **Daily Chart** — 7/30 day token or cost bar chart with Y-axis labels
- **Activity Graph** — GitHub-style contribution heatmap with 2D/3D toggle and year navigation
- **Period Navigation** — Browse weekly/monthly totals with `< >` arrows
- **Model Breakdown** — Input/Output/Cache ratio visualization
- **Cache Efficiency** — Donut chart showing cache hit ratio
- **Tray Cost** — Today's cost displayed next to tray icon (macOS: menu bar title, Windows: tooltip)
- **4 Themes** — GitHub (green), Purple, Ocean, Sunset — with dark mode support
- **Screenshot** — Capture the app window to clipboard
- **Clipboard Export** — Copy usage summary as markdown
- **Leaderboard** — Compare usage with other users (GitHub OAuth, opt-in)
- **Auto-hide** — Window hides when clicking outside

## Install from Source

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) toolchain
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code) installed and used at least once

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
3. Switch between **Overview**, **Analytics**, and **Leaderboard** tabs

### Tabs

| Tab | Content |
|-----|---------|
| **Overview** | Today's summary, 7-day chart, weekly/monthly totals, 8-week heatmap |
| **Analytics** | Full-year activity graph (2D/3D), 30-day chart, model breakdown, cache efficiency |
| **Leaderboard** | Rank your usage against other users (opt-in) |

### Settings

Click the gear icon (top right) to configure:
- **Theme**: GitHub / Purple / Ocean / Sunset
- **Number Format**: Compact (377.0K) vs Full (377,000)
- **Tray Cost**: Show/hide today's cost in system tray
- **Leaderboard**: Opt-in to share usage data + GitHub sign-in

### Leaderboard

1. Enable "Share Usage Data" in Settings
2. Click "Sign in with GitHub"
3. View Today / This Week rankings in the Leaderboard tab

Shared data: daily token count, cost, messages/sessions (no code or conversation content is shared)

## Data Source

The app reads `~/.claude/projects/**/*.jsonl` files directly to aggregate token usage.
Supplementary session/tool call counts come from `~/.claude/stats-cache.json`.

**Network requests**: Only when leaderboard is opted-in (sends aggregated data to Supabase).
Without leaderboard, the app runs completely offline.

## Architecture

```
┌──────────────────────────────┐
│  Frontend (React 19 + Vite)  │
│  ├── PopoverShell            │
│  ├── TabBar (3 tabs)         │
│  ├── TodaySummary            │
│  ├── DailyChart (SVG)        │
│  ├── ActivityGraph (2D/3D)   │
│  ├── Heatmap                 │
│  ├── ModelBreakdown          │
│  ├── CacheEfficiency         │
│  └── Leaderboard             │
├──────────────────────────────┤
│  Backend (Tauri v2 / Rust)   │
│  ├── JSONL Session Parser    │
│  ├── File Watcher (notify)   │
│  ├── Tray Icon + Cost Display │
│  └── Preferences (JSON)      │
├──────────────────────────────┤
│  Data Source                  │
│  └── ~/.claude/projects/     │
│      └── **/*.jsonl           │
└──────────────────────────────┘
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
