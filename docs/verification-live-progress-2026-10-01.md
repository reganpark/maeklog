# 긴 작업 진행 표시·요청형 요약 후보 검증 — 2026-10-01

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

후속: 2026-10-02 사용자 승인 후 Codex 0.160.0과 Maeklog 실행본에 [실제 적용](verification-live-progress-2026-10-02.md)했다. 아래는 10월 1일 적용 전 후보 검증 기록이다. 당시 `/tmp` 후보가 정리되어, 적용 때는 persistent target에서 같은 Maeklog hash를 재생성해 확인했다.

구현과 실행 후보 검증을 완료했다. **실행 중 Maeklog 패널, 설치된 main/hook 및 Codex 실행 링크에는 적용하지 않았다.** 실제 외부 모델 호출과 Luna 품질 평가도 하지 않았다. 개발·독립 검토에는 GPT-6.1 Sol / max 서브에이전트 1개를 사용했다.

## 최종 동작

- 기존 Codex TUI의 공개 중간 설명·계획·도구 생명주기를 별도 로컬 캐시에 수집한다. 자동 진행 표시의 추가 모델 호출은 0회다. 시간 기반 요약 타이머가 없다.
- `s`, 우클릭 ‘여기까지 요약’, CLI `catch-up`으로만 요청을 만든다. 요청 시점의 기록을 고정하고 생성 중 반복 요청을 묶는다. 같은 기록 버전의 준비된 결과는 추가 호출 없이 재사용한다.
- 잠정 결과는 별도 파일에 두며 기존 대기열·처리 커서·결정/완료 원장 및 주 Codex 대화에 넣지 않는다. 최종 답변이 들어오면 진행 중 생성 결과를 취소·폐기하고 기존 완료 요약을 우선한다.
- 수집 generation, 대화·턴·현재 연결·브랜치 문맥을 검사한다. 다른 탭·하위 대화·내부 요약기의 기록은 받지 않는다. 새 대화에 아직 프롬프트가 없어도 이전 진행 및 잠정 요약을 현재 작업으로 표시하지 않는다.
- 이전 작업의 늦은 progress/start/Stop/Interrupt가 현재 탭의 cwd를 되돌리지 못한다. 수집된 이전 최종 답변의 원래 근거 문맥은 보존한다. 중복 최종 훅은 없는 sequence를 새 종료 경계로 등록하지 않는다.
- 요청형 요약은 기존 GPT-6 Luna / medium, default tier, Fast 꺼짐을 사용한다. 읽기 전용·일회 실행·내부 훅 비활성화, 180초 제한과 기존 전체 3개 / Space당 1개 호출 제한을 공유한다. 실패 시 자동 재호출하지 않는다.

[사용 흐름과 한계](live-progress.md) · [브릿지 계약](../integrations/codex/live-progress.md)

## Maeklog 검증

격리된 `CARGO_TARGET_DIR=/path/to/private/validation-artifact`로 `./scripts/check`를 실행했다. 최종 formatter, clippy 전체 target `-D warnings`, **단위 128개**, 최적화 release 빌드, `cargo audit --deny warnings`가 통과했다. 최종 `git diff --check`도 통과했다.

최종 후보 실행 파일로 다음 12개 비모델 통합 스크립트를 실행하여 모두 통과했다.

| 검사 | 확인 |
|---|---|
| `test-live-progress.mjs` | 자동 0회, 원장/대기열 불변, 이벤트 중복, 고정 요청·연속 요청 병합·캐시, 최종 답변 취소/기존 요약, PID/하위 대화 거부, opt-out 세대, 늦은 다른 cwd와 최종 훅, 대화 전환 뒤 늦은 start, 중복 최종 전달 |
| `test-codex-live-progress.py` | 실제 새 Codex TUI → 새 Maeklog, 완료 전 commentary·구조화 계획 수집, 응답 종료·최종 답변 제외·정상 detach |
| `test-client-routing.mjs` | 공유 서버 환경 무시, 실제 PID·프로세스 검증, 탭 분리, 전환·원래 턴의 최종 수집, 오래된 detach, 이동·교체·연결 종료 |
| `test-tab-context.mjs` | 두 패널·탭별 기록 및 표시·열기 중복 방지·이동·끄기 |
| `test-opt-out.mjs` | 실행 중 취소, 삭제 없는 끄기, 최근 3턴만 재활성화, 늦은/다른 Stop 제외 |
| `test-recovery.mjs` | 묶음 경계·열린 턴 보존·재시도 상한·기존 결과 보존·복구·진단 |
| `test-daily-policy.mjs` | 오늘 최신 완료 우선, 과거/시각 미확인 자동 호출 없음, 실제 b/Esc/Enter 및 NO_COLOR |
| `test-branch-memory.mjs` | 누적 기억·원본 시각·스크롤/크기/정정/취소/삭제 조건 및 비변경 탐색 |
| `test-panel-menu.mjs` | 실제 우클릭·취소·숨기기·탭별 끄기 및 터미널 복원 |
| `test-parallel.mjs` | 동시 3개·Space별 단일 호출, 재시도/장애 분리, coordinator 종료·재시작 중복 방지 |
| `test-reducer-diagnostics.mjs` | 근거 검증 실패 진단·잘못된 결과 미확정·정상 인용·추가 보정 호출 없음 |
| `test-decision-contract.mjs` | 확정 근거·요청/결정 구분·재시도 피드백·작업별 실패 분리 |

업데이트 도구의 모의 테스트 8개와 격리 HOME 설치기 검사도 통과했다. 같은 upstream 버전이라도 브릿지 패치 hash가 바뀌면 이전 실행 파일을 ‘현재 설치’로 간주하지 않는 검사를 추가했다.

중간 전체 검사에서 기존 scheduler의 즉시 슬롯 재획득 단위 검사가 1회 실패했다. 그 검사 단독 실행과 최종 전체 실행은 통과했다. 일시 실패의 원인은 확정하지 않았고 production 슬롯 동작이나 해당 검사를 완화하지 않았다.

## 실제 Codex 브릿지 검증

새 Codex 0.159.3 TUI를 native PTY에서 실행하고, 정확한 공식 0.159.3 app server와 localhost SSE 응답 fixture를 연결했다. Herdr는 격리된 가짜 topology를 사용했으며 실제 pane을 만들거나 조작하지 않았다. 새 Codex home만 사용했고 사용자 인증·설정·대화를 복사하지 않았다.

첫 fixture 응답에서 공개 commentary와 실제 `update_plan` 호출을 만들고 최종 응답을 보류했다. 그 사이 Maeklog의 캐시에서 설명 및 `completed`/`inProgress` 계획을 확인했다. 최종 응답을 풀자 턴이 닫혔고 `final_answer`는 중간 설명에 들어가지 않았다. 정상 종료의 detach도 확인했다. 이 native 검사는 훅을 끈 상태이며 실제 훅의 수집·일반 요약 전환은 별도의 Maeklog CLI 검사에서 확인했다.

Codex 소스 검사는 다음과 같다.

- 브릿지 집중 검사 **6개 통과**: 기존 attach/switch/detach 및 새 공개 항목 필터·크기 제한·활성 주 대화·고정 cwd·늦은 이벤트·대기열/합치기·느리고 실패하는 실행 파일.
- `just fix -p codex-tui`, 최종 `just fmt`: 통과. 이후 TUI 테스트를 다시 실행하지 않았다.
- 전체 TUI: **5,580개 실행, 5,537개 통과, 42개 실패, 1개 시간 초과, 별도 8개 skip**. 전체 통과로 표시하지 않는다.
- 실패 38개는 [앞선 정확한 공식 0.159.3 원본 대조](verification-codex-0.159.3-2026-10-01.md)에서 같은 실패로 확인한 release 버전 문자열·정렬 snapshot이다. 기존 `.snap.new` 38개를 byte 단위로 유지했다.
- 나머지 4개는 수정하지 않은 custom_terminal의 색/커서 검사이며 이번 환경의 `NO_COLOR=1` 조건과 일치했다. 같은 조건의 진단은 [0.159.2 검증](verification-codex-0.159.2-2026-09-30.md)에 있다. 이 4개에 대한 정확한 0.159.3 원본 재실행은 이번에 하지 않았다.
- 시간 초과한 analytics 검사는 앞선 0.159.3 검증에서 단독 실행 53.837초로 통과한 같은 항목이다. 이번 전체 검사 시간 초과 자체는 남아 있다.
- 패치는 공식 commit `01fc69f4026735edfdf6789820549727a4867b11`에 깨끗하게 적용되어 의도한 11개 TUI 파일과 일치했다. Cargo.lock 및 snapshot 변경은 패치에서 제외했다.
- release CLI 빌드가 재시도에서 17분 10초에 통과했다. 첫 빌드가 최종 CLI 단계에서 외부 SIGTERM/exit 143으로 끝났고 원인은 확정하지 않았다. 작업자가 Rust 빌드 프로세스를 종료하지 않았다.
- 같은 공식 0.159.3 패키지의 helper/resource 43개를 byte/hash로 대조했다. global npm의 다른 버전 resources를 사용하지 않았다.

## 화면

모의 데이터로 최종 Maeklog 바이너리를 실제 52열·28열 PTY에서 실행했다. ANSI 프레임의 터미널 좌표·문자·색을 그대로 이미지로 렌더하여 폭별 줄바꿈, 계획·도구·잠정 구분 및 `s` 결과 재사용을 확인했다. 사용자 작업 화면이나 새 디자인 목업이 아니다.

[진행 표시](captures/live-progress/panel-working.png) · [좁은 패널](captures/live-progress/panel-narrow.png) · [요청형 요약 및 결과 재사용](captures/live-progress/panel-summary.png)

## 실행 후보와 적용 경계

| 후보 | 경로 / SHA-256 |
|---|---|
| Maeklog 0.14.0 개발 후보 | `/path/to/private/validation-artifact` · `e8b49887c64d1202312bb1366d1062ec8d890b293954de5fcc10b20ef29b79d1` |
| Codex CLI 0.159.3 | `/path/to/private/validation-artifact` · `582f93d13666aec5e601e60a5e1a9083bbace7b900d801580ee83da20d748124` |
| Codex 브릿지 패치 | `integrations/codex/codex-0.159.3-maeklog.patch` · `6eed02ae3e2650f4ddf25c38f5ff30eefd4b6b27c9d67c92bb74a1d02cd05259` |

Maeklog source/binary 검증 manifest는 `/path/to/private/validation-artifact`, Codex build/resource manifest는 같은 디렉터리의 `codex-candidate-manifest.json`이다. 최종 CLI fixture 근거는 `/path/to/private/validation-artifact`, native Codex 근거는 `/path/to/private/validation-artifact`에 보관했다.

실제 HOME에서 설치기의 **dry run만** 실행했다. 제안 패키지는 `codex-0.159.3-maeklog.2`이며 기존 registry를 유지한다. 설치된 Codex 링크와 실행 파일은 이전 `codex-0.159.3-maeklog.1` 및 hash 그대로임을 확인했다. config 쓰기, 설치, 실행 중인 Codex/공유 daemon/Maeklog 패널 재시작은 하지 않았다.

실사용 적용에는 새 Maeklog main/panel·hook·진행 요약 schema와 새 Codex 패키지가 함께 필요하다. routing 설치기만 실행하면 Maeklog 패널은 갱신되지 않는다. [product brief](product-brief.md)의 운영 실행 파일 교체와 실제 모델 평가는 별도 사용자 결정이라는 기준에 따라 후보를 검증한 뒤 이 경계에서 멈췄다. 기존 Codex 작업은 정상 종료 후 같은 터미널에서 재개해야 새 브릿지를 사용한다.

잠정 요약은 수집한 제한된 공개 기록을 대상으로 한다. 사용자 요청으로 요약한 내용을 최종 요약이 일부 다시 읽을 수 있으며, 의미 정확도·실제 비용 및 품질은 모의 검사로 보증하지 않는다.
