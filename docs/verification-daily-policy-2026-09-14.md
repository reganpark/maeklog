# Maeklog v0.12.0 — 오늘 자동 / 이전 날짜 수동

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

2026-09-14. 사용자가 새 정책의 구현·적용을 승인했고 별도 기록 백업은 생략하도록 했다. 기록 삭제, 원격 변경·병합·배포, 실제 모델 품질 평가는 요청하지 않았다.

## 동작

- 수집된 오늘의 완료 대화만 최신 묶음부터 자동 처리한다. 처음 열기·포커스·SessionStart·재활성화로 전체 transcript를 가져오지 않는다.
- 이전 날짜, 시각 미확인/자정에 걸친 대화는 자동 대상이 아니다. b 또는 Maeklog 우클릭 메뉴 → 날짜 선택 → Enter로 현재 대기 이벤트 집합을 명시적으로 요청한다.
- 날짜 화면 열기/이동/취소와 h 기록 펼침은 모델·GitHub 호출을 하지 않는다.
- 재활성화는 수집된 오늘 최근 최대 3턴만 허용한다. 나머지 오늘 원문도 삭제하지 않고 같은 날짜의 수동 보류로 남긴다. 시작 입력은 이벤트당 2,400자·묶음 합계 14,400자 이내로 제한한다.
- 정확한 처리 ID와 CurrentState를 selected-current.json에 원자적으로 저장한다. 높은 sequence까지 삭제하지 않으므로 최신순 처리·동시 새 입력·중간 실패 뒤에도 과거 원문이 남는다.
- 결정·완료 원장, 출처·시각·변경/후속 관계는 유지한다. 늦게 처리한 과거 결과는 더 최신 상태를 덮어쓰지 않는다. 최신 보고 뒤에 처리한 이전 완료 근거도 원본 순서대로 연결한다.
- 새 입력이 와도 이전 작업의 재시도 한도는 daily-retries.json에 남는다. 날짜/상태를 분리해 ‘오늘 대기·요약 중·실패·이전 날짜 사용자 요청 대기’를 보여준다.
- 자정 시점을 자동 작업 lease의 종료 조건으로 둔다. 자정을 기다린 실기 검증은 하지 않았으며 전송된 모델 요청/비용을 회수할 수는 없다.

## 검증 환경과 결과

Linux / rustc 1.97.1 / Node.js v24.18.0 / Python 3. 빌드 target은 /path/to/private/validation-artifact 격리했다.

- cargo fmt --all --check: 통과
- cargo clippy --locked --all-targets -- -D warnings: 통과
- cargo test --locked: **90 passed, 0 failed**
- 격리 release 빌드: 통과
- RustSec cargo-audit 0.22.2 --deny warnings: **통과**, 1,244 advisories / 36 crate dependencies. 검사 도구와 advisory DB는 임시 경로에 설치했다. Maeklog 런타임 의존성 추가 없음.
- 비모델 release 통합: recovery / daily-policy / parallel / opt-out / panel-menu / branch-memory **6개 모두 통과**. 모델과 GitHub는 mock executable로 대체했으며 실제 모델 평가를 실행하지 않았다.

핵심 새 검사: 오늘/과거/unknown/자정 경계 분류, 최신 8개 묶음 먼저 처리, sparse receipt 뒤 오래된 inbox가 남는 중간 실패 복구, 고정된 날짜 요청 범위, Space 끄기에 따른 요청 무효화, 최근 3턴 재활성화와 원문 보존, 재시도 한도 유지, 실제 PTY의 b/Esc/Enter와 h/scroll/resize.

구버전 테스트 중 ‘자동 전체 이력 복원’, ‘최신 미리보기 후 같은 대화 재요약’, ‘재시작 시 나머지 대기열 삭제’ 기대값은 의도적으로 새 정책으로 교체했다. legacy 포맷과 v0.11 원장 보존 테스트는 유지했다. 테스트는 JSON 저장 기준 파일이 먼저 바뀌고 원문 파일이 뒤따르는 정상 원자 저장 순서를 기다리도록 보완했다.

## 화면

모의 데이터로 실제 Maeklog 바이너리를 실행한 PTY 프레임이다. 실제 사용자 대화를 문서에 복제한 캡처는 아니다.

- [기본 상태 PNG](captures/daily-policy/daily-default.png)
- [날짜 선택 PNG](captures/daily-policy/daily-picker.png)
- [기본 출력](captures/daily-policy/daily-default.txt)
- [기본 ANSI](captures/daily-policy/daily-default.ansi), [날짜 선택 ANSI](captures/daily-policy/daily-picker.ansi)

Impeccable harden 기준으로 기존 터미널 스타일을 유지하며 한글 줄바꿈, 빈 목록, 미확인 시각, 요약 실패, 키보드 취소/명시 실행을 구분했다. 새 디자인 시스템은 도입하지 않았다.

## 실제 적용 기록

적용 대상 main/hook 실행 파일은 v0.10.0이었다. 켜져 있던 Maeklog Space만 임시 중지했고 기존 꺼짐 설정은 유지했다. demo-shop-2 Maeklog이 종료되면서 후속 close API에 pane_not_found가 한 번 반환됐으나 해당 Maeklog 프로세스 종료와 꺼짐 상태를 확인했다. 주 Herdr/Codex 프로세스는 종료하지 않았다.

실제 대기열 확인 결과 demo-space에 오늘 46턴, 이전 날짜 78턴이 있었다. 재활성화 직후 오늘 최근 3턴만 자동 대상이며 오늘의 나머지 43턴 + 이전 날짜 78턴, 총 **121턴이 수동 보류**된 것을 확인했다. 이후 새 오늘 대화가 들어오며 자동 대상이 4턴으로 늘었다. 최근 3턴 제한은 재활성화 시 기존 대기열에 적용하며 이후 새 대화 수집을 막지는 않는다. 실제 이전 날짜 수동 요약은 요청하지 않았다.

검증한 release SHA-256:
7c8eb01cb49f5c285e71bd5bc5b75691940570db13cdf32fdd4a3be4e581b35b

**로컬 적용 완료.** 아래 두 실행 파일의 v0.12.0 버전과 위 SHA-256 일치를 확인했다.

- `$HOME/repos/maeklog/target/release/maeklog`
- `$HOME/.config/herdr/plugins/config/maeklog/bin/maeklog-hook`

기존 활성 Space <runtime-id-8>/<runtime-id-9>/<runtime-id-7>/<runtime-id-10>/<runtime-id-1>의 수집 설정을 복구했다. 기존 비활성 <runtime-id-11>/<runtime-id-6>/<runtime-id-12>와 보관된 <runtime-id-13>는 변경하지 않았다. 원래 표시 중이던 활성 Maeklog 패널만 <runtime-id-14>로 복구했고, 실제 실행 경로와 화면의 `r 오늘 재시도 · b 날짜 선택` 안내를 확인했다. 사용자 포커스와 다른 pane은 유지했다. demo-space의 정상 오늘 자동 처리를 재개했으며 이는 모델 품질 평가가 아니다.

별도 Maeklog 기록 백업 파일을 만들지 않았고 저장된 기록도 삭제하지 않았다. hooks.json 재등록과 신뢰 설정 변경 없이 기존 hook 실행 파일만 교체했다. 적용 전후 설정 해시는 동일하다.

- hooks.json SHA-256: `d042b423c5112b3e4031466088c1e4091e1b2afe4fce4518d7186c8ec19c66d0`
- hook-runtime.json SHA-256: `bec9a5adb80fd4b779dc04a09fb095da3be9fea0fef7881e0474bca0aafd2c39`

## 제한

- 과거 전체 요약 기능 대신 수집된 대기열의 날짜 선택만 제공한다. 수집하지 않은 Codex 원본을 자동 복원하지 않는다.
- 모델의 의미 판단 품질은 mock 테스트가 보증하지 않는다. 이번 검사는 실제 모델 평가가 아니다. 재개 후 정상 자동 사용과 평가 호출은 구별한다.
- macOS 실기, 실제 자정 대기, 모든 전원 차단 지점, 대규모 장기 원장의 성능은 별도 검증 대상이다.
- 날짜는 시스템 date와 로컬 시간대에 의존한다. 확인할 수 없는 원본 시각을 현재 날짜로 대체하지 않는다.
- 기존 문서 중 v0.11 검증 기록은 당시 결과로 보존한다. 현재 동작은 README와 이 기록이 우선한다.
