# 이슈·PR 연결 적용·검증 — 2026-10-11

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

**10월 11일 이슈·PR 연결 기능을 Maeklog 표시 패널 5개와 helper 실행 파일 3개에 적용 완료했다.** 재부팅으로 없어진 임시 근거를 다시 준비하고, 10월 9일 검증한 후보와 정확히 같은 실행 파일을 복원해 적용했다. 아래는 당시 후보 검증과 이번 복원·실제 적용 결과를 구분한 기록이다.

## 구현 동작

현재 GitHub 원격 저장소·브랜치와 일치하는 열린 PR이 하나면 백그라운드에서 자동 연결하고, 이슈는 선택된 PR의 연결 이슈에서 가져온다. 로컬의 새 커밋은 같은 브랜치의 연결을 끊지 않는다. 여러 후보, 연결 없음, 조회 실패와 자동 연결 불가를 구분하며 번호·제목·URL을 표시한다.

`l`에서 후보 선택 또는 확인된 저장소의 번호·정확한 전체 참조를 입력하고, `o`로 링크를 연다. 기본 화면의 `R`과 연결 화면의 `r`/`R`은 재조회한다. 수동 선택·연결 안 함은 유지하며 `u`로 자동 연결을 다시 사용한다. 조회는 읽기 전용이고 중복·TTL·실패 재시도 간격을 적용한다. 렌더링과 `h`/`b`/`e` 이동은 GitHub 조회를 요청하지 않으며, 기본 화면의 `r`은 기존 대화 요약 재시도 동작이다. [상세 사용법](usage.md#이슈pr).

## 10월 9일 확인한 결과

| 검증 | 당시 결과 |
|---|---|
| Rust 단위 테스트 | 160개 PASS |
| 엄격 검사 | fmt·clippy `-D warnings` 등 strict checks PASS |
| 이슈·PR 전용 통합 검사 | PASS — 귀속·수동 선택·캐시·중복 억제·실제 PTY 조작 검증 |
| 기존 회귀 검사 | 6개 PASS |
| UI 검토 | 넓은/좁은 입력, 조회 중·연결·없음·불가·오류·후보·메뉴 등 12상태 PASS, 제품 사용을 막는 결함 없음 |

당시 캡처 환경은 `NO_COLOR=1`이어서 ANSI에 색상 SGR이 없었다. 메뉴 PNG의 일부 한글 잔상은 캡처 렌더러가 2셀 문자의 덮어쓰기를 제대로 처리하지 못한 결과였으며, 실제 ANSI 메뉴 문자열은 정상이었다. 좁은 입력 화면의 footer 마지막 한 글자 잘림은 Esc/Enter 식별과 조작을 막지 않았다. 이 한계를 제품 결함과 구분해 검토했다.

당시 첫 적용 시도는 실패하여 **실행 파일과 설치 정보를 원복**했다. 새로 열린 패널 하나는 남아 있었으므로 전체 패널 구성이 원복됐다는 뜻은 아니다. 10월 11일 재부팅 뒤에는 패널 5개가 모두 이전 실행 파일로 정상 동작함을 확인하고 이번 적용을 진행했다.

## 10월 11일 복원·실제 적용 결과

| 항목 | 확인 결과 |
|---|---|
| 제품 소스 | 지난 PASS와 동일, UI 재디자인 없음 |
| 후보 | `target/work-links-20261011/release/maeklog`, version `0.14.0` |
| 후보·설치 SHA-256 | `0330276b39c5e7b742163a4ceb0e386006b11f94a5a2a3a7fd1283acf3882248` — 이전 PASS 후보와 정확히 동일 |
| 복원 검사 | fmt·clippy `-D warnings`·160 unit·release PASS; 중단됐던 release만 이어서 완료 |
| 실제 적용 | 새 Maeklog 패널 5개 모두 후보 hash, 정상/non-deleted exe, worker 경로 존재, 같은 탭의 UI 등록 확인 |
| helper·설치 정보 | helper 3개 갱신, 기존 manifest의 helper hash 항목 2개 갱신; Codex `0.161.0`의 `installation_current=true` |
| 기존 데이터·설정 | 원장 106개와 기존 기록 1,248개 보존, 보호 설정 동일 |
| 포커스 | 비Maeklog 사용자 pane `<runtime-id-40>` → `<runtime-id-3>` 이동 기록; 새 Maeklog 5개는 모두 unfocused |

실제 표시에서 demo-shop·demo-space의 3개 패널은 ‘연결된 이슈 없음 / 열린 PR 없음’을 보여 줬다. business·maeklog의 2개는 ‘이슈/PR 자동 연결 불가’와 ‘GitHub 저장소 또는 브랜치 미확인 · 명시적 연결 필요’를 보여 줬다.

근거: 복원 검사 (local historical artifact; not bundled), 실제 적용 결과 (local historical artifact; not bundled), 실행·UI·표시 상태와 5개 화면 경로 (local historical artifact; not bundled). 전용 통합·기존 6회귀·12상태 UI 검토 결과는 같은 후보를 검증한 10월 9일 기록을 따른다. 이번 복원에서 `Cargo.lock`이 같아 cargo audit는 재실행하지 않았다.
