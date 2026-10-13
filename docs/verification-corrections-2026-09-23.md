# v0.13.0 사용자 정정 검증

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

- 단위 테스트 107개 통과: 원문/시각 보존, 저장된 정정·여러 단계 되돌리기, 같은 출처 재생 방지, 다른 Space의 오래된 편집 거부, 정정 전 모델 응답 차단, 완료 오기록 제외, 편집 확인/취소.
- fmt와 clippy `-D warnings` 통과. release 빌드는 `/path/to/private/validation-artifact`에 격리.
- 비모델 통합 8개 통과: branch-memory, daily-policy, panel-menu, recovery, parallel, opt-out, decision-contract, reducer-diagnostics. branch-memory의 실제 PTY에 근거 보기, 한글 bracketed paste, 확인 전 비저장, 취소, 저장, 제외, 되돌리기, 80/30열 검사를 추가.
- 합성 데이터 화면: `docs/captures/corrections/record-evidence.png`, `correction-confirm.png`, `correction-narrow.png`. 사용자 실제 대화 캡처가 아님.
- 실제 모델 품질 평가는 수행하지 않았음. 자동 전체 과거 검색이나 진행·막힘의 소급 정정은 없음.
- 후속 사용자 요청인 탭별 패널·컨텍스트 분리 전에 소스를 `/path/to/private/validation-artifact`로 보존. 이 검증 단계에서는 실행 중 main/hook 및 패널을 교체하지 않음.

의존성 검사: 임시 설치한 cargo-audit 0.22.2로 36개 의존성 점검, `--deny warnings` 통과.
