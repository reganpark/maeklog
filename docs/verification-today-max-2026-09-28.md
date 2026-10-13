# 오늘 완료 전체 표시와 Luna Max

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

현재 설정은 아래 후속 적용에 기록된 **GPT-6 Luna / medium · Standard(Fast 꺼짐)**이다. Max/Fast 내용은 변경 이력이다.

2026-09-28 사용자 요청으로 기본 완료 목록의 최근 3건 제한을 없앴다. 로컬 날짜가 오늘인 완료 보고를 모두 표시하고, 이전 날짜·시각 미확인 기록은 `h`의 이전 기록에 유지한다. 기준은 요약 생성 시간이 아닌 저장된 항목의 근거 시각이다. 자정 이후 패널 갱신 시 새 날짜를 기준으로 분류한다. 기록 삭제·재요약은 수행하지 않는다.

요약 모델은 `gpt-6-luna`를 유지하고 `model_reasoning_effort="max"`로 변경했다. 로컬 Codex 모델 목록에서 지원 여부를 확인했다. 기존 결정 인정 조건, 원문 근거 선택, 재시도 정책, read-only sandbox 및 hooks 비활성 설정은 유지한다. 실제 결정 요약 품질 개선은 아직 평가하지 않았으며, 설정 변경만으로 보장하지 않는다.

## 검증

- 단위 테스트 113개 통과, fmt/clippy 및 release build 통과.
- 오늘 완료 6건 전체 노출, 다른 날짜·시각 미확인 분리, 펼침 시 중복 없음, 날짜 변경과 날짜 확인 실패 검사.
- 이전 기록 페이지 계산과 보존, 과거 완료의 후속 실패 관계 검사를 새 표시 규칙에 맞춰 확인.
- branch-memory 통합: 오늘 완료 7건 전체 표시, 실제 PTY의 스크롤·접기·크기 변경, 기록 불변 확인.
- decision-contract 통합: 결정 인정 및 근거 검증 유지, 모의 호출의 Luna/max 전달 확인.
- client-routing 통합: 공유 서버와 실제 탭 연결 회귀 검사 통과.
- 별도 실제 모델 호출을 통한 품질 평가는 수행하지 않았다.

## 설치

별도 `/path/to/private/validation-artifact`에서 빌드한 뒤 기존 main/hook을 백업했다. Maeklog hide/open 액션으로 열린 패널만 갱신했으며, Codex 작업 pane은 종료하거나 재개하지 않았다.

- 적용 SHA-256: `b0b2ea78c2d231c6b9985b5628b69e94b60403db000bf7862e5fb529229b6447`
- 백업: `/path/to/private/installation-backup`
- 갱신 탭: `<runtime-id-18>`, `<runtime-id-19>`, `<runtime-id-20>`, `<runtime-id-2>`, `<runtime-id-4>`
- 각 실행 중 Maeklog 프로세스의 `/proc/PID/exe` hash와 새 화면의 ‘오늘 완료’ 제목을 확인했다.
- 수집 설정인 participation 파일들과 Codex config/hooks 파일은 적용 전후 동일함을 확인했다.

새 요약부터 max 설정을 사용한다. 이미 저장된 요약을 이 변경으로 소급 재작성하지 않는다. 기존 Codex 클라이언트 연결 패치는 그대로 사용한다.

## 후속 적용: Fast

같은 날 사용자 요청으로 Maeklog 요약 호출에 `service_tier="fast"`와 `features.fast_mode=true`를 추가했다. `gpt-6-luna`와 추론 수준 `max`는 유지한다. CLI 0.157.1은 `fast`를 요청 값 `priority`로 변환한다. 사용자 전역 Codex 설정은 수정하지 않았다.

fmt/clippy와 release build, branch-memory 및 decision-contract 통합 검사를 통과했다. 모의 모델 호출에서 Luna/max/Fast 설정이 함께 전달되는 것을 검증했다. 실제 서버의 Fast 수락 여부나 응답 속도는 별도로 측정하지 않았다.

- Fast 적용 SHA-256: `9a385fa3f8d5b799ec6f9a3842c3ee7ffb85291d1835bb6c8f5c57c96f7a13a4`
- 백업: `/path/to/private/installation-backup`
- main/hook과 열린 Maeklog 패널 5개의 실행 파일 hash를 확인했다. 수집 설정과 Codex config/hooks는 적용 전후 동일하다.
- 실행 중이던 기존 worker가 정상 완료할 수 있도록 교체 전 실행 파일은 원래 디렉터리 안에 `.before-fast-*` 이름으로 보존했다. 기존 worker는 이전 설정으로 마치며, 새 worker부터 Fast 설정으로 실행된다.

## 후속 적용: medium 복귀와 Fast 해제

사용자 요청으로 `model_reasoning_effort="medium"`, `service_tier="default"`, `features.fast_mode=false`를 명시했다. 오늘 완료 전체 표시와 기존 수집·원문·판정 계약은 유지한다.

fmt/clippy, release build와 branch-memory 및 decision-contract 통합 검사를 통과했다. 모의 호출에서 Luna/medium/Standard와 Fast 비활성 전달을 확인했다.

- 적용 SHA-256: `c28b785ce80ccb434dec57afccb6b908551ef4c33d259ded9be319b746c56abf`
- 백업: `/path/to/private/installation-backup`
- main/hook과 기존 5개 탭의 Maeklog 패널을 갱신하고 실행 중 바이너리 hash를 확인했다. 수집 설정과 전역 Codex 설정·훅 파일은 전후 동일하다.
- 이전 실행 파일을 사용하는 coordinator만 새 버전으로 재시작했다. 독립 실행 중인 단일 요약 worker는 유지했으며, 새 coordinator는 기존 worker의 잠금이 해제된 뒤 해당 탭의 후속 작업을 처리한다. 이미 시작한 모델 요청의 설정은 소급 변경하지 않는다.
