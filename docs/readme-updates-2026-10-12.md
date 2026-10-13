# Previous README updates — through 2026-10-12

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

2026-10-12 Codex **0.162.1 연동 빌드 설치 완료**: 패치를 새 upstream에 이식하고 bridge·lint·release·native 진행/작업 보고·실제 Herdr lifecycle 검사를 통과했습니다. `codex`와 `codex-maeklog` 모두 0.162.1이며, 실제 `codex update`는 이미 검증된 설치로 확인됩니다. 설정·훅과 설치 직전 Codex 프로세스 11개, 공유 서버를 보존했습니다. 전체 upstream TUI suite는 실행하지 않았습니다. [검증·설치 기록](verification-codex-0.162.1-2026-10-12.md).

2026-10-12 Codex 업데이트 런처 적용: Herdr 터미널에서 **`codex update`**로 검증된 연동 빌드를 업데이트하고, `codex update --check`로 버전을 확인합니다. 기존 0.161.0 바이너리·훅·설정을 보존하며 런처만 적용했습니다. 공유 서버와 열린 작업은 계속 실행됩니다. 격리 회귀 검사 40개와 기존 Maeklog routing 검사 PASS. 새 릴리스는 패치·빌드·연동 검사에 통과해야 설치됩니다. [검증·적용 기록](verification-codex-update-launcher-2026-10-12.md) · [설치·업데이트 설명](../integrations/codex/README.md).

2026-10-12 메뉴 키 변경: **Maeklog 기본 화면에서 `m`으로 메뉴 열기**. 하단을 `m 메뉴`로 바꾸고 Maeklog 10개에 적용했습니다. 관련 단위 51개와 실제 메뉴·77열/30열 검증을 통과했습니다. [검증·적용 기록](verification-ui-menu-2026-10-12.md).

2026-10-12 단축키 안내 수정: 하단에서 누락된 **`r 반영`**을 추가하고 30열에서도 보이도록 했습니다. Maeklog 10개 실제 하단에서 표시를 확인했습니다. [검증·적용 기록](verification-ui-status-2026-10-12.md#r-하단-안내-추가).

2026-10-12 후속 적용: **상단 안내·진행 설명의 서식 정리, 수집 공백 오표시 수정, `r` 반영 안내 보완**. `r`은 수집된 오늘 미반영 완료 대화를 반영·재시도하며, 대상 없음과 응답 대기를 구별해 안내합니다. Maeklog 10개에 적용하고 실제 demo-business 화면, 기록 1,327건·설정·작업 참조 보존을 확인했습니다. 단위 231개·통합 6개와 실제 화면 7장 검토 PASS. [검증·적용 기록](verification-ui-status-2026-10-12.md).

2026-10-12 추가 적용: **회색 글씨와 본문 중간 생략을 없애고, 시간·내용을 묶은 목록으로 정리**. 최신 공개 설명과 현재 기록·행동·조건을 기본 화면에서 끝까지 스크롤해 읽으며 하단에 남은 내용을 안내합니다. 실행 중인 Maeklog 10개에 적용했고 실제 표시 색·기록 1,319건·설정·포커스 보존을 확인했습니다. 최종 단위 221개·통합 5개와 실제 화면 4장 검토 PASS. [검증·적용 기록](verification-ui-reading-2026-10-12.md).

2026-10-12 1차 적용(이전 화면): **현재 작업·진행과 저장된 결정·완료를 두 구획으로 정리**. 당시 Maeklog 9개에 반영했습니다. `h`는 전체 기록 열람, `F10 → 현재 진행 상세`는 수집된 진행 설명 열람으로 바꿨습니다. [당시 검증·적용 기록](verification-ui-clarity-2026-10-12.md).

2026-10-02 적용: **Codex 0.160.0 + 긴 작업의 진행 표시 + 요청형 ‘여기까지 요약’**. `codex`와 `codex-maeklog`, Maeklog main/hook 및 열린 표시 패널 4개를 갱신했습니다. 공개 중간 설명·계획·도구 활동은 추가 모델 호출 없이 표시하고, `s`나 우클릭 ‘여기까지 요약’을 눌러야 잠정 요약을 만듭니다. 동일 기록은 결과를 재사용하고 최종 답변은 기존 완료 요약으로 처리합니다. 이미 열린 Codex는 작업 후 정상 종료하고 같은 터미널에서 `codex resume`으로 재개하세요. [동작과 한계](live-progress.md) · [Maeklog 적용 기록](verification-live-progress-2026-10-02.md) · [CLI 적용 기록](verification-codex-0.160.0-2026-10-02.md).

2026-10-01: **Codex 0.159.3 연동 빌드 적용 · 한 명령으로 업데이트**. `codex`와 `codex-maeklog`를 함께 갱신하고 기존 0.159.2 서버와의 연결 및 설치된 CLI의 시작·전환·재개·수집을 실제 Herdr에서 확인했습니다. 다음 업데이트는 Herdr 터미널에서 `maeklog-update --apply`로 진행할 수 있습니다. 패치 확인·빌드·연동 검사·백업·설치를 묶었으며, 패치 충돌이나 적용 전 검사 실패 시 현재 버전을 유지합니다. 컴파일과 충돌 버전의 수동 이식은 계속 필요합니다. 이미 열린 Codex는 정상 종료 후 같은 터미널에서 `codex resume`으로 재개하세요. [검증·적용 기록](verification-codex-0.159.3-2026-10-01.md) · [업데이트 명령 설명](../integrations/codex/README.md).

2026-09-30: **Codex 0.159.2 연동 빌드 적용**. 최신 버전에 터미널별 대화 연결 기능을 이식하고 시작·재개 시 화면 인식 지연도 보완했습니다. `codex`와 `codex-maeklog`가 같은 새 빌드를 사용합니다. 이미 열린 Codex는 작업 후 정상 종료하고 같은 터미널에서 `codex resume`으로 재개하세요. 다음 npm 업데이트는 연동 빌드를 자동 갱신하지 않으므로 버전에 맞춘 이식·검증이 필요합니다. [검증·적용 기록](verification-codex-0.159.2-2026-09-30.md).

2026-09-28: **오늘 완료 전체 표시 · GPT-6 Luna / medium · Fast 꺼짐**. 오늘 날짜의 완료는 개수 제한 없이 표시하고, 이전 날짜·시각 미확인 완료는 `h`에서 확인합니다. Maeklog 호출에 `service_tier="default"`와 `features.fast_mode=false`를 명시합니다. [검증·적용 기록](verification-today-max-2026-09-28.md).

2026-09-28 공유 서버 대응: Codex 터미널 클라이언트가 대화 ID를 Herdr에 직접 등록하고 Maeklog 훅이 해당 연결로 탭을 찾도록 로컬 설치했습니다. 이미 열린 Codex는 정상 종료 후 `codex resume`으로 한 번 다시 열어야 적용됩니다. 공유 서버는 계속 사용합니다. [구현·검증·적용 기록](verification-client-routing-2026-09-28.md) · [Codex 소스 패치와 업데이트 제한](../integrations/codex/README.md).

2026-09-25 후속 수정: 빈 줄과 600자 초과 범위가 선택되지 않도록 원문 후보 ID를 고르는 v6 계약을 main/hook과 열린 Maeklog 패널 6개에 적용했습니다. 단위 112개·비모델 통합 5개 통과. [변경과 검증](verification-evidence-candidates-2026-09-25.md).

2026-09-23 후속 수정: 요약 근거를 AI가 다시 쓰지 않고 원문 구간으로 선택하는 v5 출력 계약을 main/hook에 적용했습니다. 단위 112개·비모델 통합 4개 통과. 실행 중 표시 패널은 재시작하지 않았습니다. [변경과 검증](verification-evidence-spans-2026-09-23.md).

이전 모델 설정은 **GPT-6 Luna / medium**이었습니다. 2026-09-23 사용자 요청으로 추론 수준을 변경하고 main/hook 및 실행 중인 Maeklog 패널 5개에 적용했습니다. 모의 호출에서 모델 ID와 medium 전달을 확인했습니다. 적용 SHA-256: `8104a88b5ebbdb2ce9335dc187d76b03c1aba5c408d7cd6fa3199ed047f79794`.

현재 적용 버전 **v0.14.0 — 탭별 Maeklog 맥락과 사용자 정정**. 각 탭에서 `prefix + m`으로 Maeklog을 열고, `e`로 결정·완료의 근거 확인·정정·되돌리기를 할 수 있습니다. 같은 브랜치여도 다른 탭의 대화는 섞지 않습니다. [탭별 동작과 기존 기록](tab-context.md) · [정정 사용 방법](user-corrections.md).

2026-09-23 v0.14.0 적용: main/hook과 실행 중 Maeklog 5개를 갱신했습니다. 단위 109개·비모델 통합 9개·fmt/clippy·의존성 검사 통과. demo-space 두 탭의 동시 패널과 중복 없는 재열기를 실제 Herdr에서 확인했습니다. [최종 검증](verification-tabs-2026-09-23.md).

2026-09-23 사용자 요청으로 **GPT-6 Luna / high**를 소스·로컬 main/hook 실행 파일과 열린 Maeklog 패널 4개에 적용했습니다. 단위 테스트 103개, fmt/clippy, branch-memory 및 daily-policy 통합 검사가 통과했고 모의 호출에서 `gpt-6-luna`와 `high` 전달을 확인했습니다. 실제 모델 품질 평가는 수행하지 않았습니다. 적용 빌드 SHA-256: `55ded9b5e0f7e406ce80875d16643ce5ffcb39d3355c4d2d1b7a4dd94db1e7f9`.

이전 적용 기록: **v0.12.3 — 결정 근거 계약 및 오류 반영 재시도**입니다. 2026-09-15 사용자 승인으로 실행 중인 demo-shop·demo-space Maeklog 패널 두 개에 적용했습니다. main/hook 및 두 패널의 실행 파일 SHA-256은 `dce96e0debbbc0d91377c60468337e13cf71cc13a852b330ea721d3a7ed064ed`입니다. 기록과 Space별 수집·날짜·재시도 설정을 유지했으며 과거 전체 재요약은 실행하지 않았습니다. [v0.12.3 검증·적용·제한](verification-decision-contract-2026-09-15.md).

Herdr의 Maeklog 패널에서 탭·저장소·브랜치의 결정, 완료 보고, 현재 단계, 막힘과 다음 행동을 확인하는 플러그인입니다. 주 작업은 Codex가 하고 Maeklog은 대화를 정리합니다. 요약을 주 대화에 자동 주입하거나 다음 행동을 자동 실행·승인하지 않습니다.

이전 적용 기록: **v0.12.2 — 실패 진단 보존·최신 결정 우선·드래그 선택**. 2026-09-14 사용자 승인 후 적용했습니다. [당시 검증·적용 기록](verification-reducer-diagnostics-2026-09-14.md). Gruvbox Dark와 오늘 자동 / 이전 날짜 수동 요약 정책은 그대로입니다. [색상 적용 기록](verification-gruvbox-2026-09-14.md) · [v0.12.0 적용 기록](verification-daily-policy-2026-09-14.md).

시간 표시의 `+0900` 접미사 제거도 2026-09-14 사용자 승인 후 로컬 적용했습니다. `2026/09/14 14:30` 형식이며 저장 시각·로컬 시간대 변환은 유지합니다.

사용자 요청으로 Luna 추론 설정을 `max` → `high`로 변경했고, 후속 승인으로 2026-09-14 로컬 설치본에도 적용했습니다. 새로 시작하는 요약부터 `high`를 사용합니다. 적용 전에 이미 시작된 `max` 요약은 강제 중단하지 않았습니다.

`high` 변경 검증: 단위 테스트 94개, fmt/clippy, 격리 release 빌드와 비모델 branch-memory/daily-policy 통합 검사 통과. 모의 실행 인자에서 모델 ID 유지 및 `high` 전달을 확인했습니다. 180초 제한·동시 처리·재시도 한도는 그대로이며 실제 모델 평가는 수행하지 않았습니다. [OpenAI Docs의 Luna 지원 설정](https://developers.openai.com/api/docs/models/gpt-5.6-luna)을 확인했습니다. 검증 빌드 SHA-256: `845b89058670db2cec4f66e8e8bb71b44a11b8da60929cd56aeac5b20073ec28`.

적용 확인: main/hook 실행 파일 및 재개한 두 Maeklog pane의 실행 파일 해시가 위 검증 빌드와 일치합니다. 기존 coordinator를 교체하고 저장된 대기열의 정상 처리를 재개했습니다. 개별 실행 중 요약은 중단하지 않았으며 수집 enabled/generation·날짜 보류/수동 요청 설정과 훅 설정 해시는 적용 전후 동일합니다. 기록 삭제, 수집 재활성화, 수동 날짜 요청, 재시도 한도 초기화, 훅 재등록은 하지 않았습니다. 실제 속도·품질 개선 여부는 아직 평가하지 않았습니다.

v0.12.2 후속 변경: 근거 검증 실패의 진단 자료 보존, 최신 결정 우선 표시, Maeklog 내부 드래그 선택/복사 요청. [조사 결과와 한계](verification-reducer-diagnostics-2026-09-14.md). 실제 과거 실패 응답이 삭제되어 원인을 확정하지 못했으며, 근거 ID 전환이나 검증 완화는 하지 않았습니다.

v0.12.2는 당시 열려 있던 Maeklog 패널 3개에 적용했습니다. 당시 main/hook 빌드 해시는 `4f8395fd8b2be5a9ab20b2bc1600a6104718f22085b6a2bb883afeafb4c69ade`입니다. 단위 테스트 99개와 비모델 통합 7개 통과. [최신 결정 화면](captures/selection/panel-default.png) · [드래그 선택 화면](captures/selection/panel-selection.png).
