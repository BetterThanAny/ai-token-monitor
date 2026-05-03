# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/BetterThanAny/ai-token-monitor)](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Türkçe](README.tr.md) | [Italiano](README.it.md)**

A system tray app for macOS and Windows that tracks **Claude Code** and **Codex** token usage, cost, and activity in real time, with optional webhook alerts.

| Overview | Analytics |
| :---: | :---: |
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> |
| 오늘의 사용량, 7일 차트, 주간/월간 집계 | 활동 그래프, 30일 트렌드, 모델별 분석 |

## 다운로드

**[최신 릴리즈 다운로드](https://github.com/BetterThanAny/ai-token-monitor/releases/latest)**

| 플랫폼 | 파일 | 비고 |
|--------|------|------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac 지원 예정 |
| **Windows** | `.exe` 인스톨러 | Windows 10+ (WebView2 필요, 자동 설치) |

## 주요 기능

### 추적 & 시각화
- **실시간 토큰 추적** — Claude Code / Codex 세션 JSONL 파일을 직접 파싱해 정확한 사용량 집계
- **멀티 프로바이더** — Claude / Codex 간 자유 전환, 프로바이더별 가격 모델 적용
- **여러 설정 디렉토리** — 업무/개인 계정을 동시에 합산하도록 Claude/Codex 루트 경로를 여러 개 등록
- **일별 차트** — 7/30일 토큰 사용량 또는 비용 바 차트 (Y축 레이블 포함)
- **활동 그래프** — GitHub 스타일 컨트리뷰션 히트맵 (2D/3D 토글, 연도 네비게이션)
- **기간 네비게이션** — 주간/월간 집계를 `< >` 화살표로 과거까지 탐색
- **모델별 분석** — Input/Output/Cache 비율 시각화
- **캐시 효율** — 캐시 히트율 도넛 차트

### 소셜 & 공유
- **미니 프로필** — 활동 히트맵, 연속 접속일, 외부 프로필 링크
- **나의 AI 리포트** — 월간/연간 회고 카드 (최다 사용 모델, 가장 바쁜 하루, 연속 기록)
- **영수증 뷰** — 오늘 / 주간 / 월간 / 전체 기간의 영수증 스타일 요약
- **월급 비교기** — AI 지출을 월급 대비 비율(라떼/넷플릭스/치킨)로 환산
- **공유 & 내보내기** — 헤더 메뉴에서 마크다운 요약 복사, 스크린샷 캡처, 앱 공유 메시지 복사

### 알림
- **트레이 비용** — 오늘 비용을 트레이 아이콘 옆에 표시 (macOS 메뉴바 타이틀, Windows 툴팁)
- **웹훅 알림** — 사용량 임계치 도달 또는 리셋 시 Discord / Slack / Telegram으로 알림
- **자동 업데이터** — 인앱 업데이트 안내 + 다운로드 진행률

### 커스터마이즈
- **4가지 테마** — GitHub (초록), Purple, Ocean, Sunset + Auto/Light/Dark 모드
- **10개 언어** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **숫자 포맷** — 약식(`377.0K`) / 전체(`377,000`) 전환
- **자동 시작** — 부팅 시 자동 실행 옵션
- **자동 숨김** — 창 밖 클릭 시 자동 숨김

## 소스에서 설치

### 사전 요구사항

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 툴체인
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code) 또는 [Codex](https://openai.com/index/introducing-codex/) 중 하나 이상이 설치되어 있고 최소 1회 이상 사용한 상태

### 빌드

```bash
git clone https://github.com/BetterThanAny/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 개발 모드
npm run tauri build   # 프로덕션 빌드
```

## 사용 방법

### 기본 사용

1. 앱을 실행하면 시스템 트레이(macOS 메뉴바 / Windows 작업 표시줄)에 아이콘이 나타납니다
2. 아이콘을 클릭하면 사용량 대시보드가 표시됩니다
3. Switch between **Overview** and **Analytics** tabs

### 탭 설명

| 탭 | 내용 |
|-----|------|
| **Overview** | 오늘의 요약, 7일 차트, 주간/월간 집계, 8주 히트맵 |
| **Analytics** | 연간 활동 그래프 (2D/3D), 30일 차트, 모델별 사용량, 캐시 효율 |

### Settings

Settings is organized into three tabs:

| Tab | Options |
|-----|---------|
| **General** | Theme, language, appearance, number format, menu bar cost, launch on startup, monthly salary, optional Claude usage tracking |
| **Account** | Claude/Codex config directories |
| **Webhooks** | Discord / Slack / Telegram webhook URLs, alert thresholds, monitored windows, reset notifications |

## 데이터 소스

| 프로바이더 | 경로 | 비고 |
|-----------|------|------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | `~/.claude/stats-cache.json`에서 세션/툴 호출 수 보조. 여러 루트 지원. |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | 여러 루트 지원. |

## 아키텍처

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
│  ├── JSONL 세션 파서 (Claude/Codex)│
│  ├── 파일 감시 (notify)            │
│  ├── 트레이 아이콘 + 비용 표시     │
│  ├── 자동 업데이터                 │
│  ├── 웹훅 디스패처                 │
│  └── 설정 + 암호화 시크릿          │
├────────────────────────────────────┤
│  외부 서비스 (opt-in)              │
│  └── Discord / Slack / Telegram    │
└────────────────────────────────────┘
```

## 플랫폼 지원

| 플랫폼 | 상태 | 비고 |
|--------|------|------|
| **macOS** | 지원 | 메뉴바 통합, Dock 숨김, 트레이 비용 타이틀 |
| **Windows** | 지원 | 시스템 트레이 통합, NSIS 인스톨러, 툴팁 비용 표시 |
| **Linux** | 미테스트 | Tauri가 Linux를 지원하므로 동작 가능성 있음 |

## 후원

이 프로젝트가 유용하다면 [커피 한 잔 사주기](https://ctee.kr/place/programmingzombie)로 응원해주세요.

## 라이선스

MIT
