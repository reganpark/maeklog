# Maeklog 메뉴 키 m 적용 — 2026-10-12

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

사용자 선택에 따라 기본 화면에서 `m`을 누르면 Maeklog 메뉴가 열리도록 하고, 하단과 진행 상세 안내를 `m` 기준으로 바꿨다. 정상 폭에서는 `m 메뉴`, 좁은 화면에서는 `m`을 표시한다. 우클릭과 F10 호환 입력도 같은 메뉴를 연다. 기록 조회 화면에서는 m을 소비하고, 연결·정정 입력 화면은 기존 입력 처리를 우선한다.

적용 SHA256: `550286a10d4a17e41f9b52b9f601e88cfcc5aa204ec6080e70593083aff0a0bc`.

제품 변경은 `panel_menu.rs`의 키 연결과 `panel.rs`, `live_panel/compact.rs`의 표시 문구다. `panel/reader.rs`는 기존 조회 입력 검증에 m을 추가했다. 저장·수집·반영·모델 경로는 변경하지 않았다. 소스 검토 (local historical artifact; not bundled).

형식 검사·strict Clippy·관련 패널 단위 51개·release 빌드가 통과했다. 첫 Clippy 실행에서 테스트의 한 글자 문자열 패턴 지적을 고친 뒤 최종 검사를 통과했다. 전체 231개·백엔드 통합은 앞선 동일 백엔드 검증을 재사용했고 이번에 다시 실행한 것으로 계산하지 않았다. 빌드 기록 (local historical artifact; not bundled).

실제 바이너리를 격리 PTY에서 실행하는 검사 2개가 통과했다. m·우클릭 열기, Esc 취소, 기본 조회, 조회 중 m 입력 소비, 명시적 숨기기·수집 끄기의 분리, 77열·30열 메뉴를 확인했다. 표시 검사의 모의 모델·GitHub 호출은 0회였고 메뉴 열기·취소 시 보호한 기록·설정은 동일했다. 통합 결과 (local historical artifact; not bundled), 화면 검사 (local historical artifact; not bundled).

최종 ANSI에서 만든 화면 4장을 함께 검토했다. 일반 하단 (local historical artifact; not bundled), 좁은 하단 (local historical artifact; not bundled), 일반 메뉴 (local historical artifact; not bundled), 좁은 메뉴 (local historical artifact; not bundled). 검증용 데이터이며 사용자 대화를 공개한 캡처가 아니다.

Maeklog 10개에 적용하고 모두 새 실행 파일·UI 등록·실제 m 안내를 확인했다. helper 3개와 manifest hash 두 항목을 갱신했으며, journal 114개·기존 기록 1,332건·설정 89개 경로·작업 참조 11개 파일·포커스를 보존했다. 독립 적용 확인 (local historical artifact; not bundled).

백업: `/path/to/private/installation-backup`.
