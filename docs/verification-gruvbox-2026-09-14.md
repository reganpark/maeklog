# Gruvbox Dark 가독성 — v0.12.1 개발 검증

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

2026-09-14. 사용자가 제공한 실제 Maeklog 화면과 Herdr 설정의 `theme.name = "gruvbox"`를 기준으로 소제목·처리 상태의 시각적 위계를 개선했다. 본문을 내용별로 색칠하거나 화면 구성을 교체하지 않았다. Herdr·Impeccable colorize 스킬을 사용했다.

## 표시 규칙

| 역할 | 색 / 표시 |
| --- | --- |
| 저장소·브랜치 | fg1 `#ebdbb2`, 굵게 |
| 이슈·PR / 결정 / 최근 완료 / 진행 / 이전 기록 | aqua `#8ec07c`, 굵게 |
| 다음 행동 / 선택한 날짜 | yellow `#fabd2f`, 굵게. 선택 화살표 유지 |
| 확인된 활성 막힘의 소제목 / 재시도 대기 상태 | orange `#fe8019`, 굵게 |
| 실제 요약 실패 상태 | 밝힌 red `#fb6554`, 굵게 |
| 본문 | fg1 `#ebdbb2`, 일반 굵기 |
| 빈 기록·미확인 상태 / 시각 / 보조 안내 | gray `#a89984`, dim 속성 사용 안 함 |

Gruvbox 기본 red `#fb4934`는 bg0 `#282828`에서 작은 글씨 대비가 약 4.29:1이므로 같은 색 계열에서 밝기를 올렸다. 실제 사용되는 전경/배경 조합을 코드로 계산해 최소 4.5:1을 검사한다. 메뉴는 bg1 `#3c3836`와 기존 반전 선택을 유지한다. 본문 배경이나 Herdr 테마 설정은 바꾸지 않는다.

미확인 막힘은 주황색 경고로 단정하지 않으며 오늘 미요약 0턴도 녹색 검증 성공으로 표시하지 않는다. 확인된 막힘의 설명 본문은 여전히 중립색이다. 모델이 만든 텍스트에 특정 단어가 있다고 제목이나 오류색으로 분류하지 않는다.

문자열과 `Line.role`을 분리해 줄바꿈 전에 역할을 부여한다. 한글 제목의 후속 줄도 같은 역할이며 ANSI는 최종 대화형 터미널 출력에만 붙인다. 각 행 뒤에 색·굵기를 초기화해 다음 본문이나 스크롤에 번지지 않는다. `render-once`, `memory` 출력과 저장 데이터에는 ANSI가 없다. `NO_COLOR`도 존중하며 무색 환경에서도 제목·굵기·화살표·텍스트 상태를 유지한다.

## 실제 렌더링 캡처

격리된 Linux PTY에서 v0.12.1 바이너리와 모의 데이터로 실행한 ANSI 프레임이다. 사용자 실제 대화의 복제나 이미지 생성 모델을 사용한 시안이 아니다. 렌더 스크립트는 캡처의 RGB·굵기·반전·좌표를 해석한다. 아래 다섯 상태를 한 차례 묶어서 시각 확인했다.

- [기본 화면](captures/gruvbox/panel-default.png)
- [이전 기록 펼침](captures/gruvbox/panel-history.png)
- [30열 좁은 패널](captures/gruvbox/panel-narrow.png)
- [빈 기록 화면](captures/gruvbox/daily-default.png)
- [날짜 선택](captures/gruvbox/daily-picker.png)

각 PNG 옆에 같은 이름의 `.ansi` 원본을 보존한다. 긴 공백은 실제 110행 PTY의 고정 하단 도움말 위치를 유지한 것이다. 이전 버전 캡처는 다시 칠하거나 덮어쓰지 않았다.

## 테스트와 적용 경계

- `cargo fmt --all --check`: 통과.
- `cargo clippy --locked --all-targets -- -D warnings`: 통과.
- `cargo test --locked`: 94개 통과, 실패 0.
- 격리 release 빌드: 통과.
- RustSec `cargo-audit --no-fetch --deny warnings`: 기존 당일 DB 1,244건 / 의존성 36개 검사 통과. 런타임 의존성 추가 없음.
- 비모델 통합 6개: `test-recovery`, `test-daily-policy`, `test-parallel`, `test-opt-out`, `test-panel-menu`, `test-branch-memory` 모두 통과. 실제 PTY의 소제목 RGB·굵기/중립 본문, NO_COLOR, h/스크롤/30열 resize, b/Esc/Enter, 메뉴 취소·숨김·끄기를 검사했다. 화면 조작으로 기록이나 모델/GitHub 호출 수가 바뀌지 않는 기존 계약도 유지했다. 모델·GitHub·Herdr API는 mock을 사용하며 터미널 렌더링과 키 입력은 실제 PTY다.

개발 빌드: `/path/to/private/validation-artifact`

SHA-256: `088d32ea2271dbf82d141fa852d712d2cd5cb5f5c976b3c74f8324b9d0a0d5b6`

개발 검증 시에는 설치본을 유지했다. 이후 사용자가 “ㄱㄱ 적용해줘”로 승인하여 **2026-09-14 로컬 적용을 완료했다.** main/hook 실행 파일 모두 v0.12.1이며 위 개발 빌드 SHA-256과 동일하다.

활성 Maeklog 패널 하나(demo-shop-2)를 hide/open으로 <runtime-id-14> → <runtime-id-22> 재개했다. 수집 disable/enable은 실행하지 않아 재활성화 3턴 제한을 다시 적용하지 않았다. Space별 enabled/generation과 daily-restrictions/manual-day 해시가 전후 동일하다. 기록·대기열을 삭제하거나 초기화하지 않았다. 기존 coordinator만 교체하고 진행 중인 개별 Space 요약은 중단하지 않았으며 새 coordinator는 저장된 처리·재시도 권한으로 계속 동작한다. 실제 모델 품질 평가나 과거 날짜 요청, 재시도 한도 초기화는 하지 않았다.

새 pane의 실제 foreground executable과 Herdr `pane read --format ansi`에서 청록색·굵은 소제목, 노란색 다음 행동, 회색 메타데이터를 확인했다. 패널은 no-focus로 열었고 포커스 이동 명령을 실행하지 않았다. 시작 시 demo-space, 마지막 확인 시 demo-shop-2에 포커스가 있었으므로 전후 동일했다고 주장하지 않는다. 다른 사용자 pane은 교체하지 않았다.

훅 재등록·설정 변경 없음. 적용 전후 hooks.json SHA-256은 `d042b423c5112b3e4031466088c1e4091e1b2afe4fce4518d7186c8ec19c66d0`, hook-runtime.json은 `bec9a5adb80fd4b779dc04a09fb095da3be9fea0fef7881e0474bca0aafd2c39`로 동일하다. 원격 변경·병합·배포는 하지 않았다.

macOS, 밝은 테마 자동 전환, 색각 시뮬레이션은 검증하지 않았다. RGB 지원과 Gruvbox Dark 배경을 전제로 하며 색각에 관계없이 의미가 남도록 텍스트·위치·굵기·선택 화살표를 유지한다.

## 후속: 시간대 접미사 제거 적용

2026-09-14 사용자 요청 및 후속 적용 승인으로 항목별 `+0900` 접미사를 제거했다. 연도·날짜·시각은 유지하며 UTC epoch 저장이나 로컬 시간대/DST 변환은 바꾸지 않았다. 단위 테스트 94개, fmt/clippy, 새 release의 비모델 branch-memory PTY 통합 검사를 통과했다. macOS 실기 검증은 수행하지 않았다.

main/hook 설치본 SHA-256은 `fb08025101f3cd8793d6728b7517c6bd4c013478ffd186bc1e72f6d290b991ea`로 갱신했다. 열려 있던 demo-shop과 demo-shop-2 Maeklog 화면만 각각 <runtime-id-23>, <runtime-id-24>로 다시 열고 두 실제 화면에서 오프셋 없는 날짜·시간을 확인했다. 수집 disable/enable, 기록 삭제, 수동 날짜 요청, 재시도 초기화 없이 기존 coordinator의 정상 처리를 재개했다. Space별 enabled/generation, 날짜 보류/수동 요청 설정과 hooks.json/hook-runtime.json 해시는 적용 전후 동일했다. 이 절은 위 색상 적용 당시의 해시를 대체하는 최신 설치 기록이다.
