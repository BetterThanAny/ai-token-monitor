# Project Instructions

## GitHub Actions / CI/CD 규칙

### PR 생성 후 즉시 머지 금지
PR을 생성하면 GitHub가 자동으로 CI Tests 워크플로우를 큐에 넣는다. PR을 즉시 머지해도 이미 큐에 들어간 CI 실행은 취소되지 않아 **불필요한 워크플로우가 중복 실행**된다.

**반드시 지켜야 할 절차:**
1. PR 생성
2. CI Tests 통과 대기 (또는 불필요한 경우 수동 취소)
3. CI 완료 후 머지

**절대 하지 말 것:**
- `gh pr create && gh pr merge`를 한 줄로 연결하지 말 것
- PR 생성 직후 바로 머지하지 말 것 — 항상 CI 상태를 확인한 후 머지

### Release 워크플로우 (배포)
- 트리거: `workflow_dispatch`만 사용 (main push로는 릴리스되지 않음)
- 릴리스 명령:
  ```bash
  gh workflow run Release -f bump=patch    # 패치 (0.7.0 → 0.7.1)
  gh workflow run Release -f bump=minor    # 마이너 (0.7.0 → 0.8.0)
  gh workflow run Release -f bump=major    # 메이저 (0.7.0 → 1.0.0)
  ```
- 워크플로우가 자동으로 수행하는 것:
  1. `package.json` 버전 bump + `sync-version.mjs`로 tauri.conf.json, Cargo.toml 동기화
  2. Conventional commit 기반 체인지로그 생성 (feat/fix/perf 분류)
  3. `chore: release vX.Y.Z` 커밋 + 태그 생성 + main에 push
  4. macOS → Windows 순차 빌드 + GitHub Release 생성
- 진행 확인: `gh run watch`
- **로컬에서 수동 버전 변경 불필요** — 워크플로우가 전부 처리

### 버전 관리
- `package.json`의 version이 기준 (sync 스크립트가 tauri.conf.json, Cargo.toml에 반영)
- 로컬에서 수동 변경 시: `package.json` 수정 후 `node scripts/sync-version.mjs` 실행
- **권장**: 릴리스 워크플로우를 통한 자동 버전 관리 사용
