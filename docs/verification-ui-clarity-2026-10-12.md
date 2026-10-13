# Maeklog 화면 가독성 개선 검증 — 2026-10-12

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

현재 대화의 작업·공개 진행을 위에, 이 탭의 저장된 결정·완료를 아래에 배치하고 실행 중인 Maeklog 패널 9개에 적용했다. 정상 0건과 반복 안내는 줄이고 실제 실패·조건·미확인 상태는 범위를 붙여 유지했다. 저장 내용이나 요약 정책은 변경하지 않았다.

최종 Maeklog 실행 파일 SHA256:

```text
a1e0347d2f087d50624eb815b512af397f4088df4a1698bb0084a7d1aa120ecc
```

후보 및 검사 결과 (local historical artifact; not bundled), 최종 소스 지문 (local historical artifact; not bundled), 독립 소스 검토 (local historical artifact; not bundled)에서 동일 후보와 소스를 확인했다. 기존 작업이 있던 저장소이므로 변경 범위는 이번 작업 시작 시 캡처한 기준 파일과 비교했다.

## 화면과 동작

- 기본 화면은 현재 대화와 저장된 브랜치 기록의 두 구획이다. 최근 요청을 작업 제목으로 바꾸거나, 응답 종료·요약 큐 0건을 작업 완료로 해석하지 않는다.
- 결정·오늘 완료는 각각 최대 3건과 정확한 전체 건수를 표시한다. 짧은 창에서는 1~2건을 표시하며, 조건과 해결되지 않은 실패·막힘은 유지한다. 비슷한 보고를 임의로 합치지 않는다.
- `h`는 전체 기록의 읽기 전용 화면을 연다. 9개 범주에서 원문·조건·이전 기록·변경 및 근거·수집 정보를 읽고, Esc로 메인의 원래 스크롤 위치에 돌아온다.
- 긴 공개 진행 설명은 발췌임을 표시한다. `F10 → 현재 진행 상세`에서 현재 캐시에 수집·보존된 전체 설명을 읽는다. 저장 근거 정정은 기존 `e`에서 수행한다.
- F10 메뉴의 첫 선택은 읽기 전용 전체 기록이다. 상세 화면에서는 요약·재시도·연결 확인 단축키를 소비하며, 새 모델 요청이나 기록 변경을 만들지 않는다.
- 드래그 선택·OSC 52 복사 요청과 한글 셀 기준 줄바꿈을 유지한다. 상세 화면에서 폭을 바꿔도 같은 논리적 기록을 유지한다.

계약은 [ui-contract.md](ui-contract.md), 조작 설명은 [README](usage.md#화면과-조작)에 정리했다.

## 검증 결과

| 검증 | 결과 | 증거 |
| --- | --- | --- |
| 형식·모든 target strict Clippy·전체 unit·release | PASS, 217/217 unit | final-check.log (local historical artifact; not bundled) |
| 의존성 감사 | PASS, RustSec 1,296 advisory·36 dependency | root-audit.json (local historical artifact; not bundled) |
| 화면·전체 기록·복사·크기 변경·범위 분리 | PASS, 실제 PTY 80×40·80×42·30×28 | clarity-5/result.json (local historical artifact; not bundled) |
| 메뉴·날짜 정책·탭·실시간 진행·작업 보고·브랜치 기록·작업 연결 | 7개 통합 검사 PASS | fixtures-final.json (local historical artifact; not bundled) |
| 실제 Codex 0.161.0 주 에이전트 보고 | PASS, 최종 Maeklog 후보 | native producer (local historical artifact; not bundled) |
| 독립 시각 검토 | 대표 6개 프레임 승인 | root-visual-review.json (local historical artifact; not bundled) |
| 적용 시 입력·조회 화면 보호 | 정상 화면 3개 허용, 읽기/메뉴 화면 4개 보호 | activation-guards.json (local historical artifact; not bundled) |

실제 Codex 검증은 격리 설정·상태, 로컬 가짜 Responses 서버와 mock Herdr를 사용했다. 주 클라이언트 등록, 같은 턴의 Set/Related 보고와 receipt, 정상 완료 및 detach를 확인했으며 외부 모델이나 실제 사용자 대화를 실행하지 않았다.

첫 전체 unit 실행에서 기존 scheduler의 잠금 해제 직후 재획득 테스트가 1회 실패했다. 같은 테스트의 단독 실행과 최종 전체 217개 실행은 통과했다. 원인을 확정하지 않았고 scheduler 코드를 변경하지 않았다. 최초 실패 (local historical artifact; not bundled)와 단독 재검사 (local historical artifact; not bundled)를 보존했다.

통합 검사 초기 실패는 준비한 live 캐시의 prompt 출처, 탭이 없는 구버전 fixture의 범위 문구, 수집 공백 문구, 긴 문단의 시험 길이에 관한 기대 조건을 수정해 해결했다. 제품의 현재 대화 출처 검사나 읽기 전용 제약을 완화하지 않았다. 최종 9개 통합 검사는 같은 후보를 사용했고 실행 중 후보·검사 스크립트의 불변을 확인했다.

감사의 기본 홈 캐시 쓰기와 sandbox 네트워크가 제한되어 작업 디렉터리의 별도 DB로 재실행했다. 공개 RustSec DB와 crates.io 인덱스를 갱신한 최종 감사는 경고 없이 통과했다.

## 화면 증거

기본 화면 (local historical artifact; not bundled) · 좁은 창 (local historical artifact; not bundled) · 진행 발췌 (local historical artifact; not bundled) · 기록·주의 표시 (local historical artifact; not bundled) · 진행 상세 (local historical artifact; not bundled) · 메뉴 (local historical artifact; not bundled).

PNG는 최종 바이너리의 격리 PTY에서 캡처한 ANSI를 그린 증거이며 실제 사용자 Herdr의 스크린샷은 아니다. 시각 검토한 6개 프레임은 clarity-4에서 가져왔고, 이후 시험 문단 길이를 보완한 clarity-5에서 전체 조작 검사를 통과했다. 제품 코드는 동일하다. 기존 래스터화 스크립트의 한글 겹침 셀 처리와 빈 열 폭 보존을 수정한 뒤 같은 ANSI를 다시 그려 확인했다. 클립보드 수신 성공은 검증 범위에 포함하지 않는다.

## 실제 적용

검토한 적용 스크립트 (local historical artifact; not bundled)의 fresh dry-run을 확인한 뒤 `--apply`를 수행했다. 설치 main, runtime hook, 활성 Codex 패키지 Maeklog helper 세 파일을 백업하고 atomic 교체했다. Codex 설치 manifest는 해당 helper hash 두 항목만 바뀌었고 `installation_current('0.161.0')`는 true다.

백업: `/path/to/private/installation-backup`.

| 적용 검증 | 실제 결과 |
| --- | --- |
| 설치 helper 3개 | 모두 최종 후보 SHA와 일치 |
| 전체 live Maeklog 재열거 | 9개, 추가 0, 누락 0 |
| Maeklog process·UI·화면 | 9개 모두 최종 SHA, 삭제된 실행 경로 없음, worker 경로 존재, UI 등록과 두 구획 표시 정상 |
| 누적 기록 | journal 111개, 1,294→1,294 records; 기존 versions·corrections 보존 |
| 대화별 작업 참조 | 11→11 파일, 기존 thread·참조 보존 |
| 설정 | 보호한 89개 경로의 hash 또는 부재 상태 동일 |
| 적용 중 포커스 | `<runtime-id-39>` 전후 동일, 새 Maeklog은 모두 포커스 없이 열림 |

실제 적용 결과 (local historical artifact; not bundled), 전체 live Maeklog 후검사 (local historical artifact; not bundled), 독립 설치·manifest·보존 확인 (local historical artifact; not bundled)을 남겼다. 이후 전체 후검사 때 포커스는 `<runtime-id-40>`로 바뀐 상태였으며 새 Maeklog에 포커스를 주는 동작은 수행하지 않았다. 실행 중인 Codex와 사용자 작업 프로세스는 재시작하지 않았다.

첫 적용은 교체 전 프로세스 검사에서 단일 foreground 조건을 충족하지 않아 중단됐고, 추가 PID가 사라진 뒤에도 helper와 manifest가 바뀌지 않았음을 확인했다. 이후 사용자가 기본 Maeklog 화면을 선택한 상태가 되어, 확인한 해당 화면만 마지막에 갱신하도록 절차를 보완했다. 입력폼·메뉴·선택 영역·조회 화면 보호는 유지한다.

모든 새 Maeklog은 먼저 포커스 없이 연다. 설치·등록·보존 검증을 통과한 뒤에만, 원래 선택된 Maeklog의 같은 탭 형제 패널이 계속 선택돼 있고 오른쪽 이웃이 정확히 새 Maeklog일 때 선택 상태를 이어간다. 사용자가 다른 창으로 이동한 경우에는 포커스를 복원하지 않는다. 포커스 처리 7개 검증 (local historical artifact; not bundled)과 기존 입력 보호 7개 캡처 검증을 통과했다. 일시적인 복수 foreground는 최대 2초 다시 관측하지만, 마지막에는 동일한 단일 프로세스·인자·PID·실행 파일 검사를 충족해야 한다.

실제 적용 시에는 포커스가 이미 비Maeklog 작업창으로 이동해 이 복원 경로를 실행하지 않았다. 적용 과정에서 Maeklog 요약 요청이나 과거 재요약·수집 재활성화는 실행하지 않았다.
