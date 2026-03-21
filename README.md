# AI Token Monitor

macOS 메뉴바에서 Claude Code의 토큰 사용량과 비용을 실시간으로 추적하는 앱입니다.

<!-- TODO: 스크린샷 추가 -->

## 주요 기능

- **실시간 토큰 추적** — Claude Code 세션 JSONL 파일을 직접 파싱하여 정확한 사용량 표시
- **비용 계산** — 모델별(Opus, Sonnet, Haiku) 가격 기반 자동 비용 산출
- **일별 차트** — 최근 7/30일 토큰 사용량 또는 비용을 SVG 바 차트로 시각화
- **히트맵** — GitHub 스타일의 활동 히트맵 (최대 12주)
- **기간 네비게이션** — 주간/월간 집계를 `< >` 화살표로 과거 기간 탐색
- **모델별 분석** — Input/Output/Cache 비율 시각화
- **캐시 효율** — 캐시 히트율 도넛 차트
- **메뉴바 비용** — 트레이 아이콘 옆에 오늘 비용 실시간 표시 ($45)
- **숫자 포맷 토글** — `377.0K` ↔ `377,000` 전환
- **클립보드 내보내기** — 사용량 요약을 마크다운으로 복사
- **리더보드** — 다른 사용자와 사용량 비교 (GitHub OAuth, opt-in)

## 설치

### 사전 요구사항

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 툴체인
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code)가 설치되어 있고 최소 1회 이상 사용한 상태

### 소스에서 빌드

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor

# 의존성 설치
npm install

# 개발 모드 실행
npm run tauri dev

# 프로덕션 빌드
npm run tauri build
```

빌드된 앱은 `src-tauri/target/release/bundle/` 디렉토리에 생성됩니다.

## 사용 방법

### 기본 사용

1. 앱을 실행하면 macOS 메뉴바에 아이콘이 나타납니다
2. 아이콘을 클릭하면 사용량 대시보드가 표시됩니다
3. 탭으로 Overview / Analytics / Leaderboard 전환

### 탭 설명

| 탭 | 내용 |
|-----|------|
| **Overview** | 오늘의 요약, 7일 차트, 주간/월간 집계, 히트맵 |
| **Analytics** | 30일 차트, 모델별 사용량, 캐시 효율 |
| **Leaderboard** | 다른 사용자와 사용량 비교 (opt-in) |

### 설정

우측 상단 기어 아이콘을 클릭하여:
- **Number Format**: K/M 약식 ↔ 전체 숫자 전환
- **Menu Bar Cost**: 메뉴바에 오늘 비용 표시 on/off
- **Leaderboard**: 사용량 데이터 공유 opt-in + GitHub 로그인

### 리더보드

1. Settings에서 "Share Usage Data" 토글 활성화
2. "Sign in with GitHub" 클릭
3. Leaderboard 탭에서 Today / This Week 순위 확인

공유되는 데이터: 일별 총 토큰 수, 비용, 메시지/세션 수 (코드나 대화 내용은 공유되지 않음)

### 클립보드 내보내기

Header의 복사 아이콘을 클릭하면 현재 사용량 요약이 마크다운 형식으로 클립보드에 복사됩니다.

## 데이터 소스

앱은 `~/.claude/projects/**/*.jsonl` 파일을 직접 읽어 토큰 사용량을 집계합니다.
보조적으로 `~/.claude/stats-cache.json`에서 세션/도구 호출 수를 가져옵니다.

**네트워크 요청**: 리더보드 기능을 opt-in한 경우에만 Supabase로 집계 데이터를 전송합니다.
리더보드를 사용하지 않으면 앱은 완전히 오프라인으로 동작합니다.

## 아키텍처

```
┌──────────────────────────────┐
│  Frontend (React 19 + Vite)  │
│  ├── PopoverShell            │
│  ├── TabBar (Overview/       │
│  │   Analytics/Leaderboard)  │
│  ├── TodaySummary            │
│  ├── DailyChart (SVG)        │
│  ├── PeriodTotals            │
│  ├── Heatmap                 │
│  ├── ModelBreakdown          │
│  ├── CacheEfficiency         │
│  └── Leaderboard             │
├──────────────────────────────┤
│  Backend (Tauri v2 / Rust)   │
│  ├── JSONL Session Parser    │
│  ├── File Watcher (notify)   │
│  ├── Tray Icon + Cost Title  │
│  └── Preferences (JSON)     │
├──────────────────────────────┤
│  Data Source                 │
│  └── ~/.claude/projects/     │
│      └── **/*.jsonl          │
└──────────────────────────────┘
```

## 플랫폼 지원

| 플랫폼 | 상태 | 비고 |
|--------|------|------|
| **macOS** | 지원 | 메뉴바 통합, Dock 숨김, 트레이 비용 표시 |
| **Windows** | 예정 | 핵심 로직은 크로스플랫폼. macOS 전용 코드는 `#[cfg(target_os)]`로 분리됨 |
| **Linux** | 미테스트 | Tauri가 Linux를 지원하므로 기본 동작 가능성 있음 |

## 개발

```bash
# 개발 서버 실행 (핫 리로드)
npm run tauri dev

# TypeScript 타입 체크
npx tsc --noEmit

# Rust 빌드 체크
cargo build --manifest-path src-tauri/Cargo.toml
```

## 라이선스

MIT
