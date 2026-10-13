# v0.14.0 탭 분리 최종 검증·적용

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

사용자 정정(v0.13.0)의 검증을 마친 뒤 탭 분리 요청을 추가 구현했습니다.

- 단위 테스트 109개, fmt, clippy `-D warnings` 통과.
- 최종 release 바이너리로 비모델 통합 9개 통과: tab-context, branch-memory, daily-policy, panel-menu, opt-out, recovery, parallel, decision-contract, reducer-diagnostics.
- 두 탭 동시 열기·같은 탭 재열기·탭별 훅/기록/상태/렌더·한 탭의 hide/disable·재시작·pane 이동 후 수집 위치를 검증.
- cargo-audit 0.22.2 `--deny warnings` 통과: 36개 의존성, 1,264개 보안 권고 데이터베이스.
- 설치 전 기존 요약 worker를 기다리고 coordinator/각 저장소 worker/participation 잠금을 확보한 뒤 private 백업을 생성. 기존 기록을 재수집/재요약하거나 모델 평가를 수행하지 않음.
- main/hook과 Maeklog 패널 5개 적용. 실행 중 `/proc/PID/exe` 해시가 release와 일치.
- 실제 Herdr 확인: demo-shop <runtime-id-18>, demo-space <runtime-id-19>와 <runtime-id-20>, demo-business <runtime-id-2>, repos <runtime-id-37>에 각각 하나의 Maeklog 패널. 각 화면의 tab ID, 다른 탭 분리 표시, 근거 보기 단축키 확인. demo-space <runtime-id-20> 재열기 후 모든 Maeklog pane ID 유지.
- 기존 브랜치 원장 17개는 백업과 byte-for-byte 일치. 기존 Space 혼합 기록은 자동 탭 배정 없이 보관. 탭별 맥락은 새 수집부터 시작.

Release SHA-256: `1529769ac77d024888d225e28a126dc7502b0bb7d9b1e398b6d12e413ec645b1`.

로컬 private 복구 백업: `$HOME/.local/state/herdr/plugins/maeklog-upgrade-backup-1790127151`.
정정 단계 소스 체크포인트: `/path/to/private/validation-artifact`.

실제 모델의 의미 해석 정확도는 모의 테스트로 보증하지 않습니다. 이 검증 시점의 모델은 GPT-6 Luna / high였습니다. 이후 사용자 요청으로 medium을 적용한 기록은 README를 참고하세요.
