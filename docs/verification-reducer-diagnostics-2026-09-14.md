# 근거 검증 실패 조사 — 2026-09-14

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

## 확인한 사실과 확인하지 못한 것

사용자 요청 순서: 실패 자료 대조 → 실제 모델 없는 재현 → 확인된 원인 수정. 소스·README·SECURITY·기존 저장 구조를 확인했다. 저장소/상위 경로에 AGENTS.md는 없었다. 실행 중인 프로그램은 변경하지 않았다.

기존 오류 `journal quote not present in source event`는 빈 인용, 600자 초과, 원문 내 정확한 부분 문자열 불일치의 세 조건에 공통으로 사용됐다. 현재 구현은 응답 파일을 읽자마자 삭제하고 이후 검증을 수행했다. processing.json에는 사건 번호/길이/실행 시간/성공 여부만 남았고 runtime에도 조사 대상 실패 응답이 없었다. 따라서 실제 실패의 인용문과 모델 입력을 대조하거나 어느 조건인지 확정할 수 없다. 합성 테스트를 실제 장애 재현으로 주장하지 않는다.

코드 경로에서는 모델에 보낸 `events`와 첫 journal 검증의 `events`가 같은 함수 인수임을 확인했다. 재활성화 입력 제한은 원문 앞부분을 복사하며 원문은 유지한다. 이 경로만으로 실제 실패가 입력·검증 불일치였다고 결론내릴 근거는 없다. 결정·완료가 최근에도 저장된 사실 역시 과거 실패 원인을 설명하지 않는다.

## 이번에 고친 확인된 진단 결함

- 근거 오류를 `empty_quote`, `quote_too_long`, `quote_not_in_event`로 구분하고 사건 번호를 포함한다. 원문·인용문 자체를 공통 상태 메시지에 출력하지 않는다. 기존 거절 조건은 그대로다.
- 최종 응답의 파싱/검증 실패를 typed error로 전달하여 원래 응답 바이트와 모델 입력을 메모리에 유지한다. worker에서 당시 원문 묶음과 함께 Space 전용 `reducer-failure.json`에 저장한다.
- `decode`, `action_timing`, `summary_validation`, `journal_validation` 단계가 구분된다. 원래 응답과 처리 대상 이벤트를 보존하는 것이며 전체 프롬프트·현재 원장의 스냅샷을 저장하는 기능은 아니다. 인용 불일치의 비모델 재현에 필요한 자료를 보존한다. 모든 종류의 검증 오류를 완전히 재현한다고 보장하지 않는다.
- 마지막 실패 하나, 최대 2 MiB, 0600 파일. 초과/UTF8 실패는 본문을 생략했다는 표시만 남긴다. 이미 저장한 결정·완료를 삭제하지 않으며 진단 자료만 다음 실패로 교체한다. 성공 후에는 최근 실패 자료가 남아 과거 사건임을 단계/원문 시각으로 구별할 수 있다.
- Space clear 대상에 포함된다. 진단 저장 전에 참여 세대를 확인하고 participation lock을 잡아, 끄기/삭제 이후 민감한 파일이 다시 생기지 않도록 기존 권한 경계를 유지한다.

근거 ID 방식, 유사 문자열 자동 수용, 정상 항목 부분 커밋, 추가 보정 모델 호출, 재시도 입력 변경, 모델/추론/timeout 변경은 하지 않았다. 실제 원인이 확인되지 않았기 때문이다.

## 비모델 검사

단위 검사에서는 합성 원문과 인용으로 세 실패 조건을 분리하고, 저장한 자료를 다시 읽어 동일한 검증 오류가 재현되는지 확인했다. 정확한 줄바꿈 인용은 통과하고 공백으로 바꾼 인용은 여전히 거절한다. 모델에 전달한 제한 입력과 원문이 다를 때 두 값을 각각 보존하는지, 크기 초과가 명시적으로 생략되는지도 검사했다.

`scripts/test-reducer-diagnostics.mjs`는 격리 mock 모델로 실패 응답 보존, 0600 권한, 실패 시 원문 유지/미커밋, 정확한 인용의 성공, 추가 호출 없음, Space clear의 진단 삭제를 검사한다. 실제 사용자 원문을 fixture로 저장하지 않는다.

최종 결과: 단위 테스트 96개 통과, fmt/clippy(-D warnings) 통과, 격리 release 빌드 통과, 기존 당일 RustSec DB 1,244건/36 dependencies 검사 통과. 비모델 통합 `reducer-diagnostics`, `branch-memory`, `daily-policy`, `opt-out`, `recovery`, `parallel`, `panel-menu` 7개 모두 통과했다. 테스트 개발 중 fixture의 retry JSON 기본값, worker의 정상 종료 코드 계약, nullable model 필드 기대값을 실제 형식에 맞춰 수정한 뒤 최종 통과했다. 이 과정에서 실제 검증을 완화하지 않았다.

검증 빌드 `/path/to/private/validation-artifact` SHA-256: `3ba795f11a28e8daf547b824e66e4304e4a4cac28134f26287304f6b6264a760`.

설치 main/hook은 기존 Luna high 빌드 `845b89058670db2cec4f66e8e8bb71b44a11b8da60929cd56aeac5b20073ec28` 그대로다. 이번 진단 코드는 미적용이며, 새 진단 파일을 실제 Space에 쓰지 않았다. 실제 모델 호출·훅 재설치·원격 변경·병합·배포는 수행하지 않았다.

## 추가 질문: 드래그와 정렬

Maeklog은 crossterm 마우스 캡처를 켜고 오른쪽 메뉴/휠은 처리하지만 왼쪽 드래그 선택은 구현하지 않았다. 이것이 일반 선택을 막는 유력 경로지만 실제 Herdr 마우스 이벤트 전달을 실기로 재현하지는 않았다. 임시 복사는 [Herdr 복사 모드](https://herdr.dev/docs/keyboard/#copy-mode)의 prefix+[ → v/Space → 이동 → y/Enter를 사용할 수 있다. 전역 마우스 설정은 변경하지 않았다.

결정은 현재 근거 시각 오름차순(미확인 시각은 뒤), 완료는 source order 내림차순으로 표시한다. 따라서 17:05 결정이 17:23보다 위에 보이는 것은 실제 정렬 코드와 일치한다. 최신 묶음부터 요약하는 수집 정책과 화면 항목 정렬은 별개다. 이 질문을 별도 변경 승인으로 확대하지 않고, 표시 순서와 마우스 동작은 그대로 두었다.

## 남은 단계

실제 실패 원인 확정 및 원인에 맞춘 수정은 미완료다. 다음 단계는 진단 빌드의 별도 적용 승인 후, 기존 허용된 정상 처리에서 실패가 발생했을 때 해당 자료를 확인하는 것이다. 이를 위해 과거 대화를 강제로 재요약하거나 모델 평가를 실행하지 않는다. 실제 실패 자료를 확보하기 전에는 근본 원인이 해결됐다고 보고하지 않는다.

## 후속 승인 및 v0.12.2 실제 적용

사용자가 “모두 실행중인 패널에 변경해줘”로 승인하여 진단 보존 코드, 결정 최신순 정렬, 드래그 선택을 함께 구현·검증·적용했다. 앞 절의 미적용/정렬 미변경 상태는 승인 전 조사 당시 기록이다.

- 결정은 근거 시각 내림차순, 같은 시각은 source order 내림차순, 시각 미확인은 뒤에 둔다. 표시만 정렬하며 기록을 다시 쓰지 않는다.
- Maeklog 왼쪽 드래그를 직접 처리한다. 한글 2셀 경계도 글자를 쪼개지 않고 선택하며 선택 중/선택 유지 중에는 캡처한 화면을 고정한다. 놓으면 64 KiB 이내의 표시 행 텍스트를 OSC 52로 복사 요청한다. 클립보드를 읽지 않고, 요청 성공과 OS 클립보드 전달 성공을 혼동하지 않는다.
- Esc는 선택 해제만 수행하고 패널을 닫지 않는다. 다른 키/휠/resize/우클릭으로 선택을 해제할 수 있다. 기존 메뉴·스크롤·h/b 동작을 유지했다. 드래그 범위는 현재 viewport이고 자동 가장자리 스크롤·더블클릭 단어 선택은 구현하지 않았다.
- 재시도 대기 중에도 상세 실패 메시지를 표시한다. 추가 모델 호출이나 재시도 한도 변경은 없다.

Herdr와 Impeccable harden 스킬로 기존 TUI, 한글 셀 폭, 선택 표시와 안전한 메뉴 경계를 확인했다. 실제 PTY의 왼쪽 down/drag/up에서 선택 반전, UTF-8 ‘오늘’의 OSC 52 payload, Esc 후 pane 생존, 추가 모델/GitHub/원장 변경 없음을 검증했다. 모의 데이터의 [기본 화면](captures/selection/panel-default.png)과 [선택 화면](captures/selection/panel-selection.png)을 묶어 시각 확인했다. 사용자의 OS 클립보드 수신과 물리적 마우스 조작은 별도 수동 확인 대상이다.

최종 게이트: 단위 테스트 99개, fmt/clippy, release 빌드, 당일 캐시 RustSec audit 통과. 비모델 통합 reducer-diagnostics/branch-memory/daily-policy/opt-out/recovery/parallel/panel-menu 7개 모두 통과했다.

설치 main/hook 및 실행 중인 세 Maeklog pane 모두 v0.12.2 빌드 SHA-256 `4f8395fd8b2be5a9ab20b2bc1600a6104718f22085b6a2bb883afeafb4c69ade`와 일치한다. demo-shop <runtime-id-34>, demo-shop-1 <runtime-id-35>, demo-shop-2 <runtime-id-36>로 hide/open 재개했다. 실시간 demo-shop 결정 시각이 17:26 → 17:23 → 17:05 순으로 표시되는 것도 확인했다.

수집 enabled/generation, 날짜 보류·수동 요청 설정과 hooks.json/hook-runtime.json 해시가 전후 동일하다. 실행 중 요약을 중단하거나 수집을 disable/enable하지 않았고 기록·대기열 삭제, 훅 재설치, 재시도 초기화, 과거 날짜 요청, 원격 변경·병합·배포, 실제 모델 평가는 하지 않았다. 새로 시작하는 정상 요약부터 실패 진단 보존이 동작한다. 과거에 삭제된 모델 응답은 이번 적용으로 복원되지 않는다.

추가 질문에 대한 근거: 기존 `summarizer.rs`가 모델 응답을 읽은 직후 `remove_file`하고 그다음 검증했다. 이는 브랜치 정리 없이도 실패 응답이 사라지는 직접적인 경로다. 따라서 브랜치 정리가 실제 원인이었다고 판단할 근거는 없으며, 그 경로를 임의로 원인으로 지목하지 않았다.
