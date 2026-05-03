# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/BetterThanAny/ai-token-monitor)](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Türkçe](README.tr.md)**

A system tray app for macOS and Windows that tracks **Claude Code** and **Codex** token usage, cost, and activity in real time, with optional webhook alerts.

| Overview | Analytics |
| :---: | :---: |
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> |
| Utilizzo odierno, grafico 7 giorni, totali settimanali/mensili | Grafico attivita, trend 30 giorni, analisi per modello |

## Download

**[Scarica l'ultima versione](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)**

| Piattaforma | File | Note |
|-------------|------|------|
| **macOS** (Apple Silicon) | `.dmg` | Supporto Intel Mac in arrivo |
| **Windows** | Installer `.exe` | Windows 10+ (richiede WebView2, installato automaticamente) |

## Funzionalita

### Monitoraggio e visualizzazione
- **Monitoraggio token in tempo reale** — analizza i file JSONL delle sessioni di Claude Code e Codex per statistiche d'uso precise
- **Supporto multi-provider** — passa liberamente tra Claude / Codex, con modelli di costo specifici per provider
- **Directory di configurazione multiple** — aggrega account di lavoro e personali registrando piu percorsi root di Claude/Codex
- **Grafico giornaliero** — grafico a barre dei token o dei costi su 7/30 giorni con etichette sull'asse Y
- **Grafico attivita** — heatmap dei contributi in stile GitHub con vista 2D/3D e navigazione per anno
- **Navigazione per periodo** — esplora i totali settimanali/mensili con le frecce `< >`
- **Analisi per modello** — visualizzazione del rapporto Input/Output/Cache
- **Efficienza della cache** — grafico a ciambella con il tasso di hit della cache

### Social e condivisione
- **Report AI (Wrapped)** — scheda riepilogativa mensile/annuale con modello piu usato, giorno piu attivo e serie consecutive
- **Vista ricevuta** — riepilogo in stile ricevuta per oggi / settimana / mese / totale
- **Comparatore stipendio** — visualizza la spesa AI mensile come percentuale del tuo stipendio (caffe latte / Netflix / pollo)
- **Condivisione e esportazione** — copia il riepilogo in Markdown, cattura uno screenshot o copia un messaggio di condivisione dal menu dell'intestazione

### Notifiche e avvisi
- **Costo nel tray** — il costo odierno viene mostrato accanto all'icona nel tray (titolo nella barra dei menu macOS, tooltip su Windows)
- **Notifiche webhook** — avvisi su Discord, Slack e Telegram quando l'utilizzo supera le soglie o viene reimpostato
- **Aggiornamento automatico** — notifiche di aggiornamento in-app con barra di avanzamento del download

### Personalizzazione
- **4 temi** — GitHub (verde), Purple, Ocean, Sunset — con modalita Auto/Light/Dark
- **10 lingue** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **Formato numerico compatto / esteso** — `377.0K` vs `377,000`
- **Avvio automatico** — opzione di avvio automatico all'accensione del sistema
- **Nascondi automaticamente** — la finestra si nasconde quando si clicca al di fuori

## Installazione dal codice sorgente

### Prerequisiti

- [Node.js](https://nodejs.org/) 18+
- Toolchain [Rust](https://rustup.rs/)
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code) o [Codex](https://openai.com/index/introducing-codex/) installato e utilizzato almeno una volta

### Build

```bash
git clone https://github.com/BetterThanAny/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # modalita sviluppo
npm run tauri build   # build di produzione
```

## Utilizzo

### Uso di base

1. Avvia l'app: un'icona appare nel tray di sistema (barra dei menu macOS / barra delle applicazioni Windows)
2. Clicca sull'icona per aprire la dashboard
3. Switch between **Overview** and **Analytics** tabs

### Schede

| Scheda | Contenuto |
|--------|-----------|
| **Overview** | Riepilogo odierno, grafico 7 giorni, totali settimanali/mensili, heatmap 8 settimane |
| **Analytics** | Grafico attivita annuale (2D/3D), grafico 30 giorni, analisi per modello, efficienza cache |

### Settings

Settings is organized into three tabs:

| Tab | Options |
|-----|---------|
| **General** | Theme, language, appearance, number format, menu bar cost, launch on startup, monthly salary, optional Claude usage tracking |
| **Account** | Claude/Codex config directories |
| **Webhooks** | Discord / Slack / Telegram webhook URLs, alert thresholds, monitored windows, reset notifications |

## Fonti dei dati

| Provider | Percorso | Note |
|----------|----------|------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | Conteggi sessioni/chiamate strumenti da `~/.claude/stats-cache.json`. Supporta root multiple. |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | Supporta root multiple. |

## Architettura

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

## Piattaforme supportate

| Piattaforma | Stato | Note |
|-------------|-------|------|
| **macOS** | Supportato | Integrazione barra dei menu, nascondimento dal Dock, costo nel titolo del tray |
| **Windows** | Supportato | Integrazione tray di sistema, installer NSIS, costo nel tooltip |
| **Linux** | Non testato | Potrebbe funzionare dato che Tauri supporta Linux |

## Supporto

Se trovi utile questo progetto, considera di [offrirmi un caffe](https://ctee.kr/place/programmingzombie).

## Licenza

MIT
