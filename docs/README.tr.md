# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/BetterThanAny/ai-token-monitor)](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Italiano](README.it.md)**

A system tray app for macOS and Windows that tracks **Claude Code** and **Codex** token usage, cost, and activity in real time, with optional webhook alerts.

| Overview | Analytics |
| :---: | :---: |
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> |
| Bugünün kullanımı, 7 günlük grafik, haftalık/aylık toplamlar | Etkinlik grafiği, 30 günlük trendler, model bazlı analiz |

## İndirme

**[Son Sürümü İndir](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)**

| Platform | Dosya | Notlar |
|----------|-------|--------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac desteği yakında |
| **Windows** | `.exe` yükleyici | Windows 10+ (WebView2 gerekli, otomatik kurulur) |

## Özellikler

### İzleme ve Görselleştirme
- **Gerçek zamanlı token izleme** — Claude Code / Codex oturum JSONL dosyalarını doğrudan ayrıştırarak kesin kullanım istatistikleri sunar
- **Çoklu sağlayıcı desteği** — Claude / Codex arasında geçiş yapın; her sağlayıcı için ayrı maliyet modeli
- **Birden fazla yapılandırma dizini** — iş ve kişisel hesapları birleştirmek için birden fazla Claude/Codex kök dizini ekleyin
- **Günlük grafik** — 7/30 günlük token veya maliyet çubuk grafiği (Y ekseni etiketleriyle)
- **Etkinlik grafiği** — GitHub tarzı katkı ısı haritası, 2D/3D geçişi ve yıl gezintisi
- **Dönem gezintisi** — `< >` oklarıyla haftalık/aylık toplamları geriye doğru tarayın
- **Model bazlı analiz** — Input/Output/Cache oranı görselleştirmesi
- **Önbellek verimliliği** — önbellek isabet oranını gösteren halka grafik

### Sosyal ve Paylaşım
- **Yapay Zeka Raporu (Wrapped)** — en çok kullanılan model, en yoğun gün ve ardışık seri bilgilerini içeren aylık/yıllık özet kartı
- **Fiş görünümü** — bugün / haftalık / aylık / tüm zamanlar için fiş tarzı kullanım özeti
- **Maaş karşılaştırıcı** — aylık yapay zeka harcamanızı maaşınıza oranla görün (latte / Netflix / tavuk)
- **Paylaş ve dışa aktar** — başlık menüsünden markdown özeti kopyalayın, ekran görüntüsü alın veya uygulama paylaşım mesajını kopyalayın

### Bildirimler ve Uyarılar
- **Tepsi maliyeti** — bugünün maliyeti tepsi simgesinin yanında gösterilir (macOS menü çubuğu başlığı, Windows araç ipucu)
- **Webhook bildirimleri** — kullanım eşikleri aşıldığında veya sıfırlandığında Discord / Slack / Telegram uyarıları
- **Otomatik güncelleyici** — uygulama içi güncelleme bildirimleri ve indirme ilerleme durumu

### Özelleştirme
- **4 tema** — GitHub (yeşil), Purple, Ocean, Sunset + Auto/Light/Dark modu
- **10 dil** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **Sayı biçimi** — kısa (`377.0K`) / tam (`377,000`) arasında geçiş
- **Başlangıçta çalıştır** — açılışta otomatik başlatma seçeneği
- **Otomatik gizle** — pencere dışına tıklandığında otomatik gizleme

## Kaynaktan Kurulum

### Ön Koşullar

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) araç zinciri
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code) veya [Codex](https://openai.com/index/introducing-codex/) kurulu ve en az bir kez kullanılmış olmalıdır

### Derleme

```bash
git clone https://github.com/BetterThanAny/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # geliştirme modu
npm run tauri build   # üretim derlemesi
```

## Kullanım

### Temel Kullanım

1. Uygulamayı başlatın — sistem tepsisinde (macOS menü çubuğu / Windows görev çubuğu) bir simge belirir
2. Simgeye tıklayarak kullanım panosunu açın
3. Switch between **Overview** and **Analytics** tabs

### Sekmeler

| Sekme | İçerik |
|-------|--------|
| **Overview** | Bugünün özeti, 7 günlük grafik, haftalık/aylık toplamlar, 8 haftalık ısı haritası |
| **Analytics** | Yıllık etkinlik grafiği (2D/3D), 30 günlük grafik, model bazlı kullanım, önbellek verimliliği |

### Settings

Settings is organized into three tabs:

| Tab | Options |
|-----|---------|
| **General** | Theme, language, appearance, number format, menu bar cost, launch on startup, monthly salary, optional Claude usage tracking |
| **Account** | Claude/Codex config directories |
| **Webhooks** | Discord / Slack / Telegram webhook URLs, alert thresholds, monitored windows, reset notifications |

## Veri Kaynakları

| Sağlayıcı | Yol | Notlar |
|------------|-----|--------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | `~/.claude/stats-cache.json` üzerinden oturum/araç çağrı sayıları. Birden fazla kök dizin desteklenir. |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | Birden fazla kök dizin desteklenir. |

## Mimari

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

## Platform Desteği

| Platform | Durum | Notlar |
|----------|-------|--------|
| **macOS** | Destekleniyor | Menü çubuğu entegrasyonu, Dock gizleme, tepsi maliyet başlığı |
| **Windows** | Destekleniyor | Sistem tepsisi entegrasyonu, NSIS yükleyici, araç ipucu maliyet gösterimi |
| **Linux** | Test edilmedi | Tauri Linux'u desteklediği için çalışma olasılığı var |

## Destek

Bu projeyi faydalı buluyorsanız [bir kahve ısmarlayarak](https://ctee.kr/place/programmingzombie) destek olabilirsiniz.

## Lisans

MIT
