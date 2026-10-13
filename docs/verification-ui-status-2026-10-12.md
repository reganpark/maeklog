# Maeklog 상단 서식·수집 상태·r 반영 안내 — 2026-10-12

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

상단에서 작업 연결 안내와 진행 설명이 붙어 보이던 서식을 정리해 실행 중인 Maeklog 10개에 적용했다. 정상적인 과거 수집 범위 제한이 현재 수집 실패처럼 보이던 경고를 구분했고, `r`은 수집된 오늘 미반영 완료 대화를 반영·재시도하는 동작으로 유지했다.

검증 실행 파일 SHA256: `5233adb6bd05305398c463b41fcd9935663d1f9cf785bdb9b7975dc24018eaa1`.

## 변경 내용

- 작업 미연결·참고 후보 수·`l` 안내를 한 묶음으로 표시한다. 한 줄 간격 뒤에 진행 설명의 시각과 응답 상태를 제목으로 표시하고, 본문은 두 칸 들여써 문단을 구별한다. 본문 뒤에서 상태와 시각을 반복하지 않는다.
- 전체 과거를 복원하지 않았다는 `partial_history`와 수집 재개 이력은 `h`의 수집 범위 설명으로 남긴다. 이 값만으로 현재 수집 공백을 경고하지 않는다.
- 실제 journal에 남은 수집 중단 근거는 **과거 수집 공백**으로 표시하고 보존한다. 작업 미연결은 별도 안내로 유지한다.
- `r`은 오늘의 수집된 미반영 완료 대화에 기존 worker 경로로 반영을 요청한다. 실패 재시도 예산을 초기화하고 남은 수동 날짜 요청을 해제하는 기존 의미를 유지한다. 다른 날짜나 미수집 과거를 자동 처리하지 않는다.
- 오늘 완료 대화가 없으면 새 worker를 시작하지 않고, 완료 응답 대기 또는 `b`에서 선택할 이전/보류 대화 여부를 안내한다. 반영 요청은 완료 표시와 구별한다. 수집 중지 상태와 기록 읽기 실패는 오류로 반환한다.
- ‘마지막 반영’은 반영된 원문의 시각이다. `r`을 누른 시각으로 바꾸거나 과거 수집 공백의 근거를 지우지 않는다.

사용자가 제시한 demo-business 탭은 조사 당시 `partial_history: true`, 수집 활성, generation 0이었고 조사한 master journal 7개의 gaps가 모두 비어 있었다. 이 정상적인 범위 제한을 공백 경고로 합치던 조건을 수정했다. 변경 전 조사 (local historical artifact; not bundled).

## 소스와 검증

제품 변경은 표시·조작 모듈 7개와 `worker.rs`의 명시적 반영 요청 경로다. 수집·원본 저장·native 작업 보고·모델 설정·의존성은 변경하지 않았다. 상태 파일이나 재확인용 캐시를 추가하지 않았다. 소스 검토 (local historical artifact; not bundled).

형식 검사, strict Clippy, 전체 단위 231개, release 빌드를 통과했다. 단위 검사는 이전 실행에서 관찰된 프로세스 간 잠금 간섭을 줄이기 위해 `--test-threads=1`로 실행했다. 첫 실행의 1개 실패는 새로 합친 상태 제목이 줄바꿈되는 것을 이전 연속 문자열 비교가 처리하지 못한 것이었다. 공백 정규화로 의미 검사를 유지한 뒤 전체 231/231이 통과했다. 최종 빌드와 검사 (local historical artifact; not bundled), 최초 검사 로그 (local historical artifact; not bundled).

터미널 검사는 실제 바이너리·PTY와 격리된 임시 데이터, 모의 모델·GitHub를 사용한다. 첫 sandbox 실행은 Node `spawnSync`의 표준입력이 닫히지 않아 초기 hook에서 멈췄다. 이전 설치 바이너리와 새 바이너리 모두 같은 증상을 보였고, Python 표준입력에서는 둘 다 0.05초 이내 성공했다. 같은 격리 구조를 호스트에서 실행했다. 실행 환경 조사 (local historical artifact; not bundled).

날짜 정책 검사의 첫 호스트 실행은 처리 완료 영수증 저장 직후, 비동기 원문 정리 전에 원문을 비교했다. 영수증과 원문 정리의 완료를 함께 기다리도록 보완한 후 오늘 실패 재시도·이전 날짜 보존·수집 중지 검사를 통과했다. 제품 코드는 이 보완 중 변경하지 않았다. 진행 화면 검사는 첫 동시 실행에서 초기 PTY 출력 대기 제한에 걸려 별도 실행으로 확인했다.

| 검증 | 결과 | 증거 |
| --- | --- | --- |
| 형식·strict Clippy·전체 unit·release | PASS, 231/231, 소스 변경 없음 | 후보 (local historical artifact; not bundled) |
| 상단·전체 읽기·r 안내 | PASS, 77×23·77/80×44·30×28, 조회 중 모의 모델·GitHub 0회 | PTY 결과 (local historical artifact; not bundled) |
| 날짜 정책·r 실제 반영 | PASS, 오늘 실패 재시도 후 receipt와 inbox 정리 확인 | 날짜 정책 (local historical artifact; not bundled) |
| 수집 중지·재개 | PASS, incremental/latest 두 모드 | 수집 설정 (local historical artifact; not bundled) |
| 브랜치 기록·메뉴 | PASS | 기록 (local historical artifact; not bundled), 메뉴 (local historical artifact; not bundled) |
| 실시간 진행·잠정 요약·대화 전환 | PASS, 자동 호출 없음·캐시 재사용·세션/수집 범위 유지 | 진행 검증 (local historical artifact; not bundled) |
| 시각 검토 | 최종 ANSI에서 그린 7장 직접 검토, 제품 추가 수정 없음 | 시각 검토 (local historical artifact; not bundled) |

영향을 받는 통합 검사 6개 모두 통과했다. 최종 통합 결과 (local historical artifact; not bundled).

대표 화면: 상단 77×23 (local historical artifact; not bundled), 30열 본문 (local historical artifact; not bundled), 반영 대상 없음 (local historical artifact; not bundled), 반영 요청 (local historical artifact; not bundled). 사용자의 실제 대화를 공개한 캡처가 아니라 격리된 검증 자료다. 복사 요청 발생은 확인했으나 호스트 클립보드 수신은 독립 검증하지 않았다.

오늘 RustSec 감사는 Cargo.lock이 동일함을 확인해 재사용했다. native producer·전송 경계도 이전 검증과 소스가 동일한 범위에서만 증거를 재사용한다. 새 바이너리로 Codex native 전체 검사를 다시 실행했다는 뜻이 아니다. 이번에 바뀐 `r` 요청 경로는 새 단위 검사와 날짜 정책·터미널 검사로 확인한다. 재사용 범위 (local historical artifact; not bundled).

## 적용

최종 소스·실행 파일·화면 검토 후 새 사전 점검을 포함해 적용했다. main·runtime hook·활성 Codex 패키지의 Maeklog helper 3개를 백업 후 atomic 교체하고, Codex manifest의 해당 helper hash 두 항목만 갱신했다. 실행 중인 Codex와 사용자 작업 프로세스는 재시작하지 않았다.

백업: `/path/to/private/installation-backup`.

| 적용 확인 | 결과 |
| --- | --- |
| 설치 helper 3개 | 최종 후보 SHA와 모두 일치 |
| 전체 live Maeklog | 10개 모두 새 실행 파일·UI 등록 정상, 추가·누락 0, 삭제된 실행 경로 없음 |
| 실제 글자 색 | 10개 모두 밝은 본문 RGB, 이전 회색 RGB·Dim 없음 |
| 실제 demo-business 화면 | 연결 안내·진행 설명 제목·들여쓰기 확인, 잘못된 수집 공백 경고 없음 |
| 누적 기록 | journal 112개, 1,327→1,327건; 기존 versions·corrections 보존 |
| 대화별 작업 참조 | 11개 파일의 기존 thread·참조 보존 |
| 설정 | 적용 시작·종료 시 보호한 89개 경로의 hash 또는 부재 상태 동일 |
| 포커스 | 적용 시작·종료 시 `<runtime-id-41>` 동일, 새 Maeklog은 포커스 없이 열림 |
| Codex 설치 상태 | `installation_current('0.161.0')` true |

적용 결과 (local historical artifact; not bundled), 전체 Maeklog 후검사 (local historical artifact; not bundled), 독립 설치·기록·색상 확인 (local historical artifact; not bundled), demo-business 화면 확인 (local historical artifact; not bundled)을 남겼다. 입력·조회 화면 보호와 포커스 처리 함수는 이전 검토본과 동일함을 확인했다. 적용 코드 검토 (local historical artifact; not bundled).

적용은 사용자 중단 전에 완료됐다. 이후 `continue`로 후검사를 재개했을 때 현재 대화의 participation 파일 1개가 적용 종료 후 시각에 갱신되어 있었다. 적용 시작·종료의 hash는 동일하며, 늦게 관찰한 현재 바이트까지 동일하다고 주장하지 않는다. 현재 파일에는 재개된 대화의 open_turns가 있다. 후검사 시 다른 설정 88개, 기존 기록·정정·작업 참조, 실행 파일과 실제 표시 색은 정상임을 확인했다. 사용자가 이후 이동한 포커스는 되돌리지 않았다.

## r 하단 안내 추가

사용자가 후속 화면에서 지적한 `r` 안내 누락을 수정했다. `panel.rs`의 기본 화면 하단만 변경했으며 실제 셀 폭에 맞는 안내를 선택해 `r 반영`을 우선 유지한다. 정상 폭에서는 l/e/h/F10도 함께 표시하고, 30열에서는 행 범위·r 반영·h·F10을 확인했다. 단축키의 반영 동작과 저장·수집 코드는 바뀌지 않았다.

후속 적용 SHA256: `bd2fb688079ddc2fd178b7e1c9c15995a0d937914a9eaad1c26d1151606ce57a`.

형식·strict Clippy·관련 패널 단위 15개·release 빌드 및 실제 77열/30열 PTY 검사가 통과했다. 두 화면을 한 번에 검토했고 모의 모델·GitHub 호출은 0회였다. 앞선 231개 전체 단위·6개 통합 검사 이후 제품 변경은 하단 안내 하나이므로 이번에는 관련 검사만 실행했다. 빌드 (local historical artifact; not bundled), 소스 검토 (local historical artifact; not bundled), PTY 결과 (local historical artifact; not bundled), 77열 (local historical artifact; not bundled), 30열 (local historical artifact; not bundled).

Maeklog 10개에 적용한 뒤 모든 실제 하단의 `r 반영` 표시와 실행 파일을 확인했다. helper 3개·manifest hash 두 항목을 갱신했고, 기존 기록 1,329건·설정 89개 경로·작업 참조·포커스를 보존했다. 최초 읽기 점검 때 demo-business 선택 화면이 열려 있었으나 적용 전 다시 확인했을 때 닫혀 있었으며, 새 적용 점검에서 모두 기본 화면임을 확인했다. 독립 적용 확인 (local historical artifact; not bundled).

백업: `/path/to/private/installation-backup`.
