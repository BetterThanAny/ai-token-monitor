# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/BetterThanAny/ai-token-monitor)](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [Türkçe](README.tr.md) | [Italiano](README.it.md)**

一款 macOS 和 Windows 系統托盤應用,可即時追蹤 **Claude Code** 和 **Codex** 的權杖使用量、費用和活動,並支援可選 Webhook 提醒。

| 總覽 | 分析 |
| :---: | :---: |
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> |
| 今日使用量、7 天圖表、週/月彙總 | 活動圖、30 天趨勢、模型分析 |

## 下載

**[下載最新版本](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)**

| 平台 | 檔案 | 備註 |
|------|------|------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac 即將支援 |
| **Windows** | `.exe` 安裝程式 | Windows 10+(需要 WebView2,自動安裝) |

## 主要功能

### 追蹤與視覺化
- **即時權杖追蹤** — 直接解析 Claude Code / Codex 的工作階段 JSONL 檔案,準確統計使用量
- **多供應商支援** — 可在 Claude / Codex 之間切換,各供應商採用獨立價格模型
- **多設定目錄** — 可同時新增多個 Claude/Codex 根目錄,彙總工作與個人帳號使用量
- **每日圖表** — 7/30 天權杖或費用柱狀圖(含 Y 軸標籤)
- **活動圖** — GitHub 風格貢獻熱力圖(支援 2D/3D 切換與按年瀏覽)
- **期間導覽** — 使用 `< >` 箭頭瀏覽過去的週/月彙總
- **模型分析** — Input/Output/Cache 比例視覺化
- **快取效率** — 快取命中率環形圖

### 社交與分享
- **AI 報告 (Wrapped)** — 月度/年度回顧卡片(最常用模型、最忙碌的一天、連續紀錄)
- **收據檢視** — 今日 / 本週 / 本月 / 全部 的收據式使用摘要
- **薪資比較** — 將 AI 花費換算為月薪佔比(拿鐵 / Netflix / 炸雞)
- **分享與匯出** — 透過頂部選單複製 Markdown 摘要、擷取螢幕截圖或應用分享訊息

### 提醒
- **托盤費用** — 在托盤圖示旁顯示今日費用(macOS 選單列標題,Windows 工具提示)
- **Webhook 通知** — 用量達到閾值或重置時透過 Discord / Slack / Telegram 通知
- **自動更新器** — 應用內更新提示,含下載進度

### 自訂
- **4 種主題** — GitHub(綠色)、Purple、Ocean、Sunset,並支援自動/淺色/深色模式
- **10 種語言** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **數字格式** — 精簡(`377.0K`)/ 完整(`377,000`)切換
- **開機自動啟動** — 可選開機時自動啟動
- **自動隱藏** — 點擊視窗外自動隱藏

## 從原始碼安裝

### 先決條件

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 工具鏈
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- 已安裝 [Claude Code](https://claude.ai/claude-code) 或 [Codex](https://openai.com/index/introducing-codex/) 其中至少一個,且至少使用過一次

### 建置

```bash
git clone https://github.com/BetterThanAny/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 開發模式
npm run tauri build   # 生產建置
```

## 使用方法

### 基本操作

1. 啟動應用程式後,系統托盤(macOS 選單列 / Windows 工作列)會出現圖示
2. 點擊圖示開啟使用量儀表板
3. 在 **概覽** 和 **分析** 標籤之間切換

### 分頁說明

| 分頁 | 內容 |
|------|------|
| **總覽** | 今日摘要、7 天圖表、週/月彙總、8 週熱力圖 |
| **分析** | 全年活動圖(2D/3D)、30 天圖表、模型分析、快取效率 |

### 設定

設定分為 3 個分頁:

| 標籤 | 選項 |
|------|------|
| **一般** | 主題、語言、外觀、數字格式、選單列費用、開機自啟、月薪、可選 Claude 用量追蹤 |
| **帳戶** | Claude/Codex 設定目錄 |
| **Webhooks** | Discord / Slack / Telegram Webhook URL、提醒閾值、監控視窗、重置通知 |

## 資料來源

| 供應商 | 路徑 | 備註 |
|--------|------|------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | 從 `~/.claude/stats-cache.json` 補充工作階段/工具呼叫數。支援多個根目錄。 |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | 支援多個根目錄。既有 JSONL 不可靠記錄歷史 session 使用的是 Fast 還是 Standard，因此 ai-token-monitor 不能僅憑來源資料自動還原這個區別。 |

## 架構

```
┌────────────────────────────────────┐
│  前端 (React 19 + Vite)            │
│  ├── PopoverShell / Header         │
│  ├── TabBar (2 tabs)               │
│  ├── TodaySummary / DailyChart     │
│  ├── ActivityGraph (2D/3D) / Heatmap│
│  ├── ModelBreakdown / CacheEfficiency│
│  ├── SalaryComparator              │
│  └── SettingsOverlay (3 tabs)      │
├────────────────────────────────────┤
│  後端 (Tauri v2 / Rust)            │
│  ├── JSONL 工作階段解析器 (Claude/Codex)│
│  ├── 檔案監視 (notify)             │
│  ├── 托盤圖示 + 費用顯示           │
│  ├── 自動更新器                    │
│  ├── Webhook 分派器                │
│  └── 偏好設定 + 加密機密           │
├────────────────────────────────────┤
│  外部服務 (可選)                   │
│  └── Discord / Slack / Telegram    │
└────────────────────────────────────┘
```

## 平台支援

| 平台 | 狀態 | 備註 |
|------|------|------|
| **macOS** | 支援 | 選單列整合、隱藏 Dock、托盤費用標題 |
| **Windows** | 支援 | 系統托盤整合、NSIS 安裝程式、工具提示費用顯示 |
| **Linux** | 未測試 | Tauri 支援 Linux,基本功能可能可用 |

## 支援

如果您覺得此專案有用,歡迎 [請我喝杯咖啡](https://ctee.kr/place/programmingzombie)。

## 授權

MIT
