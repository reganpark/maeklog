# Codex 0.160.0 연동 빌드 적용 — 2026-10-02

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

`codex`와 `codex-maeklog`를 검증한 **0.160.0 연동 빌드**로 함께 갱신했다. Maeklog의 터미널별 대화 연결과 새 진행 정보 브릿지를 유지하며, 같은 버전의 공식 helper/resource를 사용한다. 설치본의 시작·전환·종료·재개·훅 수집을 실제 Herdr에서 확인했다.

기존 Codex 세션과 공유 app server는 재시작하지 않았다. 이미 열려 있던 Codex는 작업 후 정상 종료하고 같은 터미널에서 `codex resume`으로 재개해야 새 실행파일과 진행 브릿지를 사용한다.

## 적용 파일

| 항목 | 값 |
|---|---|
| 공식 릴리스 | [rust-v0.160.0](https://github.com/openai/codex/releases/tag/rust-v0.160.0) |
| upstream commit | `a956835d020762cb2b570053af06f643a11c0ecc` |
| 현재 패치 | `integrations/codex/codex-0.160.0-maeklog.patch` |
| 패치 SHA-256 | `8ecab284d75119588b4b3a118c806ff189519d19ec71cb8ec16fdd1d33adcf17` |
| 설치 패키지 | `~/.local/share/maeklog/codex-0.160.0-maeklog.1` |
| CLI SHA-256 | `1932f839f198257e6f35be86b8874a304644a6589b9906aa2b4ea76b57984e86` |
| 함께 설치한 Maeklog hook SHA-256 | `e8b49887c64d1202312bb1366d1062ec8d890b293954de5fcc10b20ef29b79d1` |
| 설치 백업 | `/path/to/private/installation-backup` |

정확한 공식 source에 기존 0.159.3 브릿지가 충돌 없이 적용됐다. 최종 포맷 이후 의도한 TUI 11개 파일의 diff를 0.160.0 패치로 저장했고, 실제 빌드 source와 패치가 byte 단위로 같으며 reverse apply check도 통과했다. Cargo.lock의 159개 local workspace 버전만 정규화했고 external dependency graph는 바꾸지 않았다. Bazel lock 동기화도 통과했다.

공식 `@openai/codex@0.160.0` 패키지를 별도 private prefix에 받았다. CLI 외 helper/resource 43개를 후보에서 원본과 hash 대조했다. 설치기는 matching package 전체를 복사하고 CLI만 연동 빌드로 교체한다. 전역 npm 설치를 변경하지 않았다.

## 검증 결과와 제한

- 브릿지 집중 검사 6개, TUI clippy, 최종 `just fix -p codex-tui`와 `just fmt` 통과. 마지막 fix/fmt 이후 Rust 테스트를 반복하지 않았다.
- 전체 TUI는 **5,611개 실행, 5,572개 통과, 39개 실패, 별도 8개 skip**이었다. 전체 통과로 표시하지 않는다.
- 38개 snapshot 실패는 보관된 공식 무패치 0.159.3 baseline의 실패 diff와 릴리스 버전 정규화 후 일치했다. 30개는 버전 치환만, 6개는 그에 따른 정렬·줄바꿈, 2개는 서비스 버전 안내 차이다. 설명되지 않은 차이는 없었다. 별도의 무패치 0.160.0 전체 실행은 하지 않았으며 snapshot을 갱신하거나 완화하지 않았다.
- 나머지 provider-history 실패는 테스트가 공유 캐시의 오래된 `debug/codex` 0.157.1을 사용한 상태였다. matching debug CLI 0.160.0을 빌드하여 같은 검사를 다시 실행했고 **10.779초에 통과**했다.
- release 빌드에 `cargo rustc --locked --release -p codex-cli --bin codex -- -C lto=off`를 사용했다. 첫 updater 실행은 release 준비 중 SIGTERM/143으로 끝났다. 보관된 source와 cache에서 복구한 최종 빌드는 **31분 42초에 통과**했다. Rust 프로세스를 강제로 종료하지 않았다.
- 격리 HOME 설치기 검사, updater 모의 검사 8개 통과. 새 Maeklog 후보의 기존 routing 검사도 통과했다.
- 실제 새 CLI와 공식 0.160.0 server를 native PTY 및 가짜 Herdr에 연결했다. 최종 응답 전에 공개 commentary·구조화 계획을 수집했고, 최종 답변 제외·턴 종료·정상 detach를 확인했다.
- 실제 Herdr의 임시 sibling pane에서 candidate와 설치본을 각각 확인했다. 공유 서버의 잘못된 pane 환경을 무시하고 실제 탭으로 prompt/final을 수집했으며, `/new`, `/quit`, 같은 대화 resume과 다음 턴 수집을 확인했다. 설치본 검사는 `--default-bridge`로 실제 설치 hook을 사용했다. 각 검사는 자기 pane/server만 정리했다.

검증은 localhost 응답 fixture와 mock만 사용했다. 외부 Luna 호출이나 실제 요약 품질 평가는 수행하지 않았다.

## 설치 확인

두 launcher의 실제 경로와 CLI hash가 같은 후보와 일치한다. matching hook, packaged hook 및 helper 실행파일 hash가 설치 manifest와 일치하며 updater의 `installation_current("0.160.0")`도 참이다. 기존 hook 명령·신뢰 hash·사용자 config를 유지했다. 이전부터 꺼져 있던 legacy Herdr SessionStart hook을 다시 켜지 않았다.

적용 후에도 기존 Codex PID 5895는 0.159.3 실행파일로 계속 살아 있었고 공유 app server PID <process-id>·<process-id>도 유지됐다. 별도 진행 기능의 Maeklog main/패널 갱신은 [Maeklog 적용 기록](verification-live-progress-2026-10-02.md)에 있다.

private 근거: `/path/to/private/validation-artifact`. source/build/검사 로그, `source-bridge-proof.json`, `snapshot-analysis.json`, `candidate-manifest.json`, `verification.json`, native fixture와 두 Herdr lifecycle 결과를 보관한다.
