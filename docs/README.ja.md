# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/BetterThanAny/ai-token-monitor)](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Türkçe](README.tr.md) | [Italiano](README.it.md)**

A system tray app for macOS and Windows that tracks **Claude Code** and **Codex** token usage, cost, and activity in real time, with optional webhook alerts.

| Overview | Analytics |
| :---: | :---: |
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> |
| 今日の使用量、7日間チャート、週間/月間集計 | アクティビティグラフ、30日間トレンド、モデル別分析 |

## ダウンロード

**[最新リリースをダウンロード](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)**

| プラットフォーム | ファイル | 備考 |
|-----------------|---------|------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac 対応予定 |
| **Windows** | `.exe` インストーラー | Windows 10+（WebView2 必要、自動インストール） |

## 主な機能

### 追跡 & 可視化
- **リアルタイムトークン追跡** — Claude Code / Codex のセッション JSONL を直接パースして正確に集計
- **マルチプロバイダー対応** — Claude / Codex を切替、プロバイダー別の価格モデルを適用
- **複数の設定ディレクトリ** — 仕事用 + 個人用アカウントを同時に集計できるよう Claude/Codex ルートを複数追加可能
- **日別チャート** — 7/30 日間のトークンまたはコストの棒グラフ（Y 軸ラベル付き）
- **アクティビティグラフ** — GitHub スタイルのコントリビューションヒートマップ（2D/3D 切替、年ナビゲーション）
- **期間ナビゲーション** — `< >` 矢印で週間/月間集計を過去までブラウズ
- **モデル別分析** — Input/Output/Cache 比率の可視化
- **キャッシュ効率** — キャッシュヒット率のドーナツチャート

### ソーシャル & 共有
- **AI レポート (Wrapped)** — 月間/年間のまとめカード（よく使うモデル、もっとも忙しかった日、連続記録）
- **レシートビュー** — 今日 / 週 / 月 / 全期間のレシート風サマリー
- **給与コンパレーター** — AI への支出を月給に占める割合（ラテ/Netflix/チキン換算）で表示
- **共有 & エクスポート** — ヘッダーメニューから Markdown サマリーのコピー、スクリーンショット、アプリ共有メッセージのコピー

### 通知
- **トレイコスト** — 今日のコストをトレイアイコンの横に表示（macOS メニューバータイトル / Windows ツールチップ）
- **Webhook 通知** — 使用量が閾値に達したり、リセットされた際に Discord / Slack / Telegram へ通知
- **自動アップデーター** — アプリ内アップデート通知 + ダウンロード進捗

### カスタマイズ
- **4 つのテーマ** — GitHub（グリーン）、Purple、Ocean、Sunset + Auto/Light/Dark モード
- **10 言語対応** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **数値フォーマット** — 短縮（`377.0K`）/ 完全（`377,000`）切替
- **自動起動** — 起動時に自動実行
- **自動非表示** — ウィンドウ外クリックで自動的に非表示

## ソースからインストール

### 前提条件

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) ツールチェイン
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code) または [Codex](https://openai.com/index/introducing-codex/) のいずれかがインストール済みで、1 回以上使用していること

### ビルド

```bash
git clone https://github.com/BetterThanAny/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 開発モード
npm run tauri build   # プロダクションビルド
```

## 使い方

### 基本

1. アプリを起動するとシステムトレイ（macOS メニューバー / Windows タスクバー）にアイコンが表示されます
2. アイコンをクリックしてダッシュボードを開きます
3. Switch between **Overview** and **Analytics** tabs

### タブ説明

| タブ | 内容 |
|------|------|
| **Overview** | 今日のサマリー、7 日間チャート、週間/月間集計、8 週間ヒートマップ |
| **Analytics** | 年間アクティビティグラフ（2D/3D）、30 日間チャート、モデル別分析、キャッシュ効率 |

### Settings

Settings is organized into three tabs:

| Tab | Options |
|-----|---------|
| **General** | Theme, language, appearance, number format, menu bar cost, launch on startup, monthly salary, optional Claude usage tracking |
| **Account** | Claude/Codex config directories |
| **Webhooks** | Discord / Slack / Telegram webhook URLs, alert thresholds, monitored windows, reset notifications |

## データソース

| プロバイダー | パス | 備考 |
|------------|------|------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | `~/.claude/stats-cache.json` からセッション/ツール呼び出し数を補足。複数ルート対応。 |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | 複数ルート対応。既存の JSONL には履歴セッションが Fast か Standard かを示す情報が確実には記録されないため、ai-token-monitor はソースデータだけからこの区別を自動復元できません。 |

## アーキテクチャ

```
┌────────────────────────────────────┐
│  Frontend (React 19 + Vite)        │
│  ├── PopoverShell / Header         │
│  ├── TabBar (2 tabs)               │
│  ├── TodaySummary / DailyChart     │
│  ├── ActivityGraph (2D/3D) / Heatmap│
│  ├── ModelBreakdown / CacheEfficiency│
│  ├── SalaryComparator              │
│  └── SettingsOverlay (3 tabs)      │
├────────────────────────────────────┤
│  Backend (Tauri v2 / Rust)         │
│  ├── JSONL セッションパーサー (Claude/Codex)│
│  ├── ファイル監視 (notify)         │
│  ├── トレイアイコン + コスト表示   │
│  ├── 自動アップデーター            │
│  ├── Webhook ディスパッチャー      │
│  └── 設定 + 暗号化シークレット     │
├────────────────────────────────────┤
│  外部サービス (オプトイン)          │
│  └── Discord / Slack / Telegram    │
└────────────────────────────────────┘
```

## プラットフォーム対応

| プラットフォーム | 状態 | 備考 |
|-----------------|------|------|
| **macOS** | 対応済み | メニューバー統合、Dock 非表示、トレイコストタイトル |
| **Windows** | 対応済み | システムトレイ統合、NSIS インストーラー、ツールチップコスト表示 |
| **Linux** | 未テスト | Tauri が Linux をサポートしているため、基本動作する可能性あり |

## サポート

このプロジェクトが役に立ったら、[コーヒーをおごる](https://ctee.kr/place/programmingzombie)で応援してください。

## ライセンス

MIT
