# Codex 0.162.1 연동 빌드 검증 — 2026-10-12

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

**당시 원래 구현의 검증·설치 완료.** `codex`와 `codex-maeklog` 모두 `codex-cli 0.162.1`을 출력한다. 실제 `codex update`도 정상 종료하며 이미 검증된 설치이므로 추가 빌드를 하지 않는다. 공유 서버와 열린 Codex 작업은 유지했다.

기존 fallback 패치가 upstream의 `startup.rs` 초기화 변경과 충돌했다. 새 0.162.1 패치는 upstream의 agents-overview grouping 초기화를 유지하면서 기존 터미널 등록·진행 브릿지를 이식한다. 검사 또는 빌드가 중단된 동안 기존 0.161.0 설치는 유지하고, 보존된 source와 Cargo cache에서 검증을 재개했다.

| 항목 | 확인 결과 |
| --- | --- |
| 공식 source | `rust-v0.162.1` / `092d3acd6bec3e3a14bdc7e7a2810ab628ab759d` |
| 패치 | `integrations/codex/codex-0.162.1-maeklog.patch` |
| 패치 SHA-256 | `965c4f7fc443f01e3d1927759ad7b994825a71ce83038f9c9ee608678256d0c2` |
| 설치 패키지 | `~/.local/share/maeklog/codex-0.162.1-maeklog.1` |
| native CLI SHA-256 | `9dcf0850d770ec1d45efb77780f5db374f007462faad76434f2d8b22827600f6` |
| stable launcher SHA-256 | `7826ab71c80b83c3792ed7b164b24f42f2b07878f866f24811cc1be5712696aa` |
| 설치 백업 | `/path/to/private/installation-backup` |

패치 사전 적용 검사와 포맷 후 reverse-apply 검사가 통과했다. 기존 0.160.0 패치와 production 추가 코드는 같고, `herdr_bridge_live_tests.rs`에 partial-answer와 root-turn 회귀 검사를 추가했다. source 변경은 의도한 TUI 11개 파일과 Cargo.lock의 local workspace 버전 정규화뿐이다. 정규화는 161개 package의 `0.0.0` 버전을 `0.162.1`로 바꾸며, 공식 source의 external dependency 버전과 graph는 유지한다. Bazel 9.2.0 lock 동기화도 통과했다.

| 검증 | 결과 |
| --- | --- |
| Herdr bridge Rust 검사 | 7개 PASS, 무관한 5,719개 skipped. 전체 TUI suite 실행 결과가 아니다. |
| TUI clippy | PASS. 수정하지 않은 upstream `app-server/src/message_processor.rs:372`의 `clippy::let_and_return` 경고 1개. |
| installer·launcher·updater 회귀 | 각각 16·11·13개, 총 40개 PASS |
| fixture title 분류 | 12개 PASS: 기존/current title metadata, user prompt-prefix, 일반 prompt·developer prefix 구분. 실제 handler를 메모리 입출력으로 확인했다. |
| fixture Unix socket | 긴 evidence 경로 3개(107·118·122바이트) 대신 0700 private `/tmp` 경로(39바이트) 사용. 실제 AF_UNIX bind와 cleanup PASS. |
| release 빌드 | PASS, optimized profile 28분 39초. 수정하지 않은 upstream compiler 경고 3개. |
| 공식 resources 설치·비교 | npm PASS. 설치된 helper/resource 파일 43개가 공식 package와 byte-identical. |
| Maeklog routing | PASS |
| 실제 Herdr lifecycle | 후보 CLI와 설치된 stable launcher 모두 시작·prompt/final hook·`/new`·종료·resume PASS |
| native live progress | `--partial-answer` PASS: 공개 중간 응답·계획 수집, 완료·detach, final-answer 제외 확인 |
| native task reports | 실제 hook context·report helper·code-mode 실행 경로 PASS |

0.162.1의 `partial_answer`는 완료 전 공개 중간 응답이며 브릿지의 기존 non-final projection에 포함된다. `final_answer` 제외를 Rust와 native fixture에서 확인했고, 새 `rootTurnId`가 실제 active turn ID를 대체하지 않도록 Rust 회귀 검사에서 확인했다. Title fixture는 `thread_source=thread_title`과 기존 `system`, 현재 title prompt-prefix를 지원하며 hidden title 요청이 progress fixture의 main-response 순서를 소비하지 않도록 한다. Native 검사는 별도 official 0.162.1 app server와 로컬 응답 fixture를 사용했으며 외부 모델을 호출하지 않았다.

Release 경고는 upstream `app-server/src/lib.rs:1512`의 `unused_mut`와 `cloud-tasks/src/lib.rs:16,17`의 unused import 2개다. 해당 파일은 패치 대상이 아니며 빌드와 lint는 정상 종료했다. 전체 upstream TUI suite를 통과했다고 주장하지 않는다.

설치 후 `installation_current=true`이며 native CLI hash가 검증 후보와 같다. `codex`는 기존 stable launcher를 유지하고 `codex-maeklog`는 새 native package를 가리킨다. `config.toml`, `hooks.json`, launcher와 설치된 Maeklog hook의 hash·경로는 설치 직전과 같다. 최신 `<validation-id>/before-install.json`에 기록한 Codex 프로세스 11개가 모두 동일 PID·start ticks로 유지됐고, 이 중 공유 서버 PID <process-id>·<process-id>도 유지됐다. 이전 중단 전 PID를 보존 결과로 사용하지 않았다. 이미 열린 클라이언트는 기존 코드를 계속 실행하므로 작업 후 정상 종료하고 `codex resume`으로 재개한다.

`codex update 0.162.1`, `codex update --check`, 인자 없는 실제 `codex update`를 설치 후 확인했다. 두 apply 명령은 `already installed; no build needed`로 정상 종료하며, check는 설치 버전과 최신 대상 모두 0.162.1로 보고한다. 새 upstream 릴리스의 패치 호환성을 보장하는 결과는 아니다.

당시 근거는 공개 사본에 포함하지 않은 사적 검증 폴더에 보존했다. 여기의 경로 표기는 실제 접근 가능한 증거 링크가 아니다. 최종 runner의 `<validation-id>/verification.json`은 `applied=true`이며, 같은 디렉터리의 `build.log`, `install.log`, `post-install.json`, `installed-live-test.log`, `update-noop.log`, `update-check.log`, `update-latest-noop.log`에 빌드·설치·사후 확인을 기록했다. [사용·설치 방법](../integrations/codex/README.md).
