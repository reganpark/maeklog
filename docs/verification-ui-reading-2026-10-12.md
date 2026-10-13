# Maeklog 본문 밝기·서식·전체 읽기 개선 — 2026-10-12

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

앞선 화면은 회색 보조 정보가 많고 진행 설명·다음 행동을 잘라 보여 주어 내용이 끝나지 않은 것처럼 보였다. 사용자가 제공한 화면을 기준으로 본문 읽기 방식과 서식을 수정해 실행 중인 Maeklog 10개에 적용했다.

최종 실행 파일 SHA256: `f44318a7ce882ecac2eca5c08cb86671a61eb536f3225039f427f91758df8653`.

## 변경 내용

- 본문·시각·빈 상태·안내·하단 조작 글자는 모두 밝은 Gruvbox 전경색을 사용한다. 회색 전경과 Dim은 사용하지 않는다. 참조·실제 경고·실패의 의미 색은 유지한다.
- 현재 작업과 저장 기록을 굵은 제목·구분선으로 구별하고, 기록 범주도 굵게 표시한다. 각 기록의 시각·보고 유형 또는 담당·본문을 글머리표 한 묶음으로 연결하고, 이어지는 줄과 조건을 들여쓴다.
- 최신 공개 진행 설명, 연결 제목, 유효 결정·오늘 완료·진행·막힘·다음 행동의 개수 제한과 본문 말줄임을 제거한다. 기본 화면을 스크롤해 조건과 끝 문장까지 읽는다.
- 마지막 반영·미반영·수집 공백은 저장 기록의 상태 안내 한 곳에 모은다. 실제 실패와 막힘은 본문·조건을 유지한다.
- 공개 설명이 있으면 최근 요청 전체는 `F10 → 현재 진행 상세`에서 읽는다. 공개 설명이 없으면 최근 요청 전체를 기본 화면에도 표시한다. 요청을 작업 제목으로 해석하지 않는다.
- 하단에는 보이는 행 범위·전체 행 수와 아래 내용 안내를 표시한다. 방향키·휠·PageUp/PageDown·Space·Home/End로 읽고, `h`와 F10 상세 조회 및 선택·복사 안내를 유지한다.

수집 단계에서 이미 생략된 기록은 생략 안내를 유지한다. 캐시에 보존된 내용을 모두 표시하는 변경이며, 수집하지 않은 과거를 복원하지 않는다. 완료 표시는 보고이고, 응답 종료나 요약 대기열 0건을 작업 완료로 바꾸지 않는다.

## 변경 범위와 검증

제품 변경은 `panel.rs`, `panel_style.rs`, `branch_panel/compact.rs`, `live_panel/compact.rs` 네 표시 모듈이다. 작업 시작 시 캡처한 기준 파일과 비교했으며 저장·수집·모델·작업 보고·동시 실행 코드와 의존성은 바뀌지 않았다. 독립 소스 검토 (local historical artifact; not bundled), 소스 고정 기록 (local historical artifact; not bundled).

형식 검사와 모든 target strict Clippy를 통과했고 전체 단위 검사는 최종 221/221 통과했다. 첫 전체 실행에서는 이전 검증에서도 관측된 `scheduler::tests::leases_enforce_three_spaces_and_one_attempt_per_space`가 잠금 해제 뒤 재획득에서 실패했다. 해당 단독 검사와 전체 재검사는 통과했다. 원인은 확정하지 않았고 scheduler 코드는 변경하지 않았다.

| 검증 | 결과 | 증거 |
| --- | --- | --- |
| 형식·모든 target strict Clippy·전체 unit·release | PASS, 최종 221/221 unit, 검사 중 소스 변경 없음 | 후보 (local historical artifact; not bundled), 전체 로그 (local historical artifact; not bundled) |
| 실제 터미널 전체 읽기 | PASS, 77×44·80×44·30×28, 본문·조건·최근 요청·조회 복귀·복사·대화 전환 | reading-4 (local historical artifact; not bundled) |
| 브랜치 기록·실시간 진행·메뉴·날짜 정책 | 4개 통합 검사 PASS | 최종 5개 통합 결과 (local historical artifact; not bundled) |
| 독립 시각 검토 | 실제 ANSI에서 만든 대표 4장 승인 | 시각 검토 (local historical artifact; not bundled) |
| 실제 설치·글자 색·기록 보존 | Maeklog 10개 모두 PASS | 독립 적용 확인 (local historical artifact; not bundled) |

첫 scheduler 실패와 재검사 증거는 최초 로그 (local historical artifact; not bundled), 단독 검사 (local historical artifact; not bundled)에 보존했다.

초기 PTY 검사는 부분 프레임·같은 높이의 이전 폭·고정 70ms 범주 이동 대기를 새 화면으로 오인했다. 최종 검사는 완성된 하단, 실제 행 위치·범주 변화, 77열 화면의 75셀 구분선 및 최대 출력 열을 확인한다. 이전 실행에서 실제 입력 반영 대기가 2,167ms인 사례를 확인해 기존 일반 대기 한도인 10초를 사용했다. 최종 실행에는 정지한 이동이 없었다. 기존 브랜치 검사의 원문 비교는 들여쓰기 공백만 양쪽에서 정규화했고 전체 문장·최신순 검증은 유지했다. 이 보완 중 제품 코드와 후보 실행 파일은 바뀌지 않았다.

수집·저장·native 보고 경계가 그대로이므로 해당 경계의 이전 Codex 0.161.0 검증을 재사용했다. 새 후보에서 native 검사를 다시 실행했다는 의미는 아니다. 오늘 수행한 RustSec 감사도 동일한 Cargo.lock인 것을 확인한 뒤 재사용했다. 변경·재사용 범위 (local historical artifact; not bundled). 이번 조회 검사는 모의 모델·GitHub 호출 0건과 기록·설정 바이트 불변을 확인했다.

## 화면 증거

기본 화면 77열 (local historical artifact; not bundled) · 마지막 행동과 조건 (local historical artifact; not bundled) · 좁은 화면 30열 (local historical artifact; not bundled) · 현재 진행 상세 (local historical artifact; not bundled).

모두 최종 후보를 격리 PTY에서 실행한 ANSI를 그린 이미지이며 검증용 데이터다. 사용자의 실제 대화를 공개한 캡처가 아니다. 4장 모두 지정한 실제 열 수로 렌더링했다. 마지막 행동 캡처는 End로 마지막 조건과 ‘끝’ 안내까지 확인했다. OSC 52 복사 요청은 검사했으나 호스트 클립보드의 수신 성공은 검증 범위에 포함하지 않는다.

## 실제 적용

최종 소스·후보·화면 검토 후 적용 스크립트 (local historical artifact; not bundled)의 새 사전 점검을 포함해 적용했다. 앞선 점검 때는 9개였고 적용 직전 새로 열린 Maeklog 한 개를 포함해 10개를 확인했다. main·runtime hook·활성 Codex 패키지의 Maeklog helper 세 파일을 백업하고 atomic 교체했으며, Codex manifest는 해당 helper hash 두 항목만 변경했다.

백업: `/path/to/private/installation-backup`.

| 적용 확인 | 결과 |
| --- | --- |
| 설치 helper 3개 | 모두 최종 후보 SHA와 일치 |
| 전체 live Maeklog | 10개 모두 최종 실행 파일·UI 등록 정상, 삭제된 실행 경로 없음, 추가·누락 0 |
| 실제 표시 색 | 10개 모두 밝은 본문 RGB 확인, 이전 회색 RGB와 Dim 없음 |
| 누적 기록 | journal 111개, 1,319→1,319건; 기존 versions·corrections 모두 보존 |
| 대화별 작업 참조 | 11개 파일의 기존 thread·참조 보존 |
| 설정 | 보호한 89개 경로의 hash 또는 부재 상태 동일 |
| 포커스 | 적용 전후 `<runtime-id-40>` 동일, 새 Maeklog은 포커스 없이 열림 |
| Codex 설치 상태 | `installation_current('0.161.0')` true |

적용 결과 (local historical artifact; not bundled), 전체 Maeklog 후검사 (local historical artifact; not bundled), 독립 helper·manifest·기록·설정·실제 색상 검사 (local historical artifact; not bundled)를 남겼다. 보호한 입력·조회 화면 검사와 포커스 처리 함수는 이전 검증 절차와 동일함을 비교했다. 이번 적용에서는 선택된 Maeklog을 복원하는 경로를 실행할 필요가 없었다. 실행 중인 Codex와 사용자 작업 프로세스는 재시작하지 않았다.
