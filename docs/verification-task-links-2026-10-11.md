# 대화 작업 연결 검증 및 로컬 적용 — 2026-10-11

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

대화별 현재 작업·관련 PR·참고 후보·이전 연결과 실제 주 에이전트 보고 경로를 구현하고 로컬 Maeklog에 적용했다. 저장된 브랜치 원장의 기존 기록을 대화 작업으로 재귀속하지 않는다.

최종 Maeklog 0.14.0 실행 파일 SHA256:

```text
f3dbb71e32469c3dc25766df16d8f1ff73312853280db3dd7d32a97288cac339
```

증거 기준은 candidate-final.json (local historical artifact; not bundled)과 최종 소스 지문 (local historical artifact; not bundled)이다. 검사·통합 실행 중 소스와 후보가 변경되지 않았으며, 각 통합 스크립트도 시작 SHA와 종료 SHA를 비교했다.

## 검증 결과

| 범위 | 결과 | 증거 |
| --- | --- | --- |
| 형식, 모든 target strict Clippy, 전체 unit, release | PASS, 205/205 unit | final-check.log (local historical artifact; not bundled) |
| 의존성 감사 | PASS, 최신 RustSec 1,296 advisory와 36 dependency | final-audit.json (local historical artifact; not bundled) |
| 참조별 TTL·backoff 단독 재현 | 연속 3회 PASS | ttl-repeats.json (local historical artifact; not bundled) |
| 실제 Codex 주 에이전트 producer | 최종 후보 PASS | native-producer-5/result.json (local historical artifact; not bundled) |
| 보고·thread·수동 정정·삭제 통합 | 최종 후보 PASS, 11개 시나리오 | task-reports-7/task-report-result.json (local historical artifact; not bundled) |
| 기존 브랜치 연결·실제 chooser 통합 | 최종 후보 PASS | work-links-5/run.json (local historical artifact; not bundled) |
| client routing, live progress, tab context, opt-out, daily policy, panel menu, branch memory | PASS | fixtures-2.json (local historical artifact; not bundled) |
| 정상·교체 실패·복수·좁은 chooser·무연결 화면, none 선택/저장 | 독립 시각 검토 PASS | ui-visual-review.json (local historical artifact; not bundled), ui-none-review.json (local historical artifact; not bundled) |

기존 회귀는 앞선 후보에서 통과한 증거를 유지했다. 그 뒤의 제품 변경은 보고 context snapshot 경합 수정과 chooser 직접 입력 중 브랜치 후보 변경 처리뿐이다. 이 두 경계는 마지막 후보의 전체 unit 및 세 통합 검사에서 다시 확인했다. `Cargo.lock`은 불변이므로 root의 별도 최신 감사 PASS를 재사용했다.

실제 설치된 Codex 0.161.0 TUI와 격리 app server, localhost 가짜 Responses 모델, mock Herdr를 사용했다. 공식 UserPromptSubmit 추가 문맥이 실제 main model request에 들어가고, workspace-write 도구가 Set→최신 receipt/revision→Related를 보고하여 패널 없이 두 processed receipt를 받았다. private Maeklog state는 sandbox 쓰기 범위 밖에 두었다. child 검사는 실행 thread ID 불일치 거부이며 실제 spawned child turn을 실행했다고 주장하지 않는다. 외부 모델 호출과 사용자 설정 변경은 없었다.

통합 검사에서는 native attach가 이미 존재하고 task 파일이 없는 상태를 새 prompt 없이 초기화했다. 실제 no-client chooser는 열기 전용이었다. 다른 탭·같은 번호의 다른 저장소·늦은 Stop/이전 prompt·수동 정정 뒤 오래된 보고·사라진 `/tmp` 큐·만료/중복/크기 제한·Stop drain 교착을 확인했다. GitHub 조회는 mock의 GET/view/list만 허용했다.

현재 이슈 142와 관련 PR 285, 검증된 인계 worktree, 별도 main 원장 화면을 확인했다. B 조회 실패는 B 의도를 유지하고 A를 이전 연결로 보존했다. none은 탐색만으로 저장되지 않고 Enter 후 저장·닫혔다. 브랜치 checkout 중 직접 입력도 캡처한 저장소의 144로 검증·저장되고 다른 브랜치 cache를 바꾸지 않았다. 명시적 private 삭제는 본인 current/retired spool의 quote와 task state를 제거하며 peer 데이터와 기존 원장 records/versions/proof를 보존했다. 기존 수집 중지 gap만 정확히 추가됐다.

## 실제 적용 및 보존

검토된 적용 스크립트 (local historical artifact; not bundled)를 기본 dry-run으로 확인한 뒤 root 승인으로 `--apply`했다. 설치 main, runtime hook, 활성 Codex 패키지의 Maeklog helper 세 파일을 백업하고 atomic 교체했다. Codex 설치 manifest는 기존 Maeklog helper hash 두 항목만 변경했다. Codex CLI와 code-mode host는 교체하지 않았고 `installation_current('0.161.0')`는 true다.

백업: `/path/to/private/installation-backup`.

| 적용 검증 | 실제 결과 |
| --- | --- |
| 설치 helper 세 파일 | 모두 최종 SHA와 일치 |
| 전체 live Maeklog 재열거 | 9개, 추가 0, 누락 0 |
| Maeklog foreground | 9개 모두 최종 SHA, `(deleted)` 없음, 실제 worker 실행 경로 존재 |
| UI 등록·실제 화면 | 9개 모두 정상, 현재 대화 작업과 저장된 브랜치 경계 표시 |
| 기존 검증 client warm 초기화 | 4개, 새 prompt 주입 없이 활성화; 나머지 5개는 active thread 없음 |
| 기록 | 실제 journal 110개, 1,266→1,266 records; 이전 versions/corrections 보존 |
| 설정 | 보호한 87개 경로의 hash/부재 상태 동일; 수집 동의·날짜·Codex config/hooks 보존 |
| 포커스 | `<runtime-id-38>` 전후 동일; 새 Maeklog 9개 모두 unfocused |

실제 적용과 보존 증거는 result.json (local historical artifact; not bundled), 전체 process/UI/화면 재검증은 all-pm-post-activation.json (local historical artifact; not bundled)에 있다. 이미 실행 중인 Codex나 사용자 pane을 재시작하지 않았다.

배포 직후 현재 참조는 모두 비어 있었고 실제 화면은 ‘지정된 이슈·PR 없음’이었다. 과거 발언을 새 현재 작업으로 추측하지 않았다. 자동 보고 문맥은 **다음 유효한 사용자 prompt부터** 주입된다. 지원 문맥이 없는 client도 수집된 명시적 prompt의 보수적 fallback을 사용하며, 모든 자연어를 이해하거나 보고 누락을 해제·완료로 간주하지 않는다. 계약과 제한은 [task-reports.md](../integrations/codex/task-reports.md)에 정리했다.
