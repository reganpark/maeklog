# 빈 근거 선택 재발 방지 — 2026-09-25

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

demo-business <runtime-id-17>의 실제 실패 진단에서 event:120의 45..45 구간이 줄바꿈 한 글자였음을 확인했다. v5는 원문 변형을 막았지만 빈 구간과 길이 초과 범위를 선택할 수 있었고, 이후 검증에서 묶음 전체가 거절됐다. 사적 원문은 저장소에 복제하지 않았다.

v6 모델 출력은 reference와 candidate_id만 받는다. 코드가 먼저 원문을 문단 단위로 나누고, 긴 문단은 Unicode 문자 600개 이하로 나눈다. 양끝 공백을 제거한 뒤 빈 후보는 제외한다. 후보는 모두 원문의 정확한 부분 문자열이다. 내부 줄바꿈, 강조와 링크는 그대로 유지한다. 모델은 구간의 시작/끝 번호를 조합하지 않고 사건에 속한 후보 ID 하나를 선택한다. 여러 근거는 기존 최대 8개 evidence 항목으로 표현한다. decision_basis에는 첫 사용자 evidence와 같은 후보를 선택하도록 지시하며, 기존 사용자 출처·의도·조건·인용 포함 검증은 유지한다.

없는 사건/후보, 잘못된 ID 자료형, quote 혼용, 이전 segment 범위 응답은 거절한다. 모델 출력 스키마는 v6이고 저장 원장 형식은 그대로다. 기존 자유 quote를 쓰는 레거시 테스트 응답은 여전히 exact journal 검증을 통과해야 한다. 후보의 의미적 적합성까지 보장하지 않으며 잘못된 후보 ID 등 다른 모델 오류는 여전히 실패할 수 있다. 긴 문단은 문장 중간에서 나뉠 수 있다.

검증 결과:

- 단위 112개, clippy(-D warnings), fmt, diff 검사 통과.
- 공백/빈 줄만 있는 입력에 후보 0개, 이전 45번째 빈 줄 재현 형태, 599/600/601/1200/1201자 경계, 다바이트 문자, 강조·링크·줄바꿈 보존 검사.
- 제공된 모든 후보가 nonblank·600자 이하·원문 부분 문자열이며 실제 resolve 가능한지 검사.
- release 빌드와 비모델 통합 reducer-diagnostics, decision-contract, branch-memory, daily-policy, tab-context 5개 통과.
- 후보 ID 오류와 구형 범위 응답은 진단을 남기고 미커밋, 정상 후보는 원문 인용으로 저장됨을 확인.

실행 중 main을 빌드로 덮지 않도록 `/path/to/private/validation-artifact`에서 빌드했다. 검증 후 main/hook을 백업하고 원자적으로 교체했다. 백업: `/path/to/private/validation-artifact`. 적용 SHA-256: `742d629821834ad262434585a82b96d04a0e2a76284f1d2fff2dbbbfcd88e79a`.

Herdr의 공식 Maeklog hide/open 액션으로 열린 패널 6개만 재개했다. <runtime-id-18>, <runtime-id-19>, <runtime-id-20>, <runtime-id-2>, <runtime-id-17>, <runtime-id-21> 모두 `/proc/PID/exe` 해시가 새 release와 일치했다. 각 Space의 participation.json은 전후 동일(원래 없는 파일은 그대로 없음)했다. 다른 작업 pane이나 agent를 닫거나 입력하지 않았다. hooks 설정 재설치, 기록 삭제, 재시도 한도 초기화, 과거 날짜 요청은 수행하지 않았다. 패널 재개에 따른 기존 허용 작업의 자동 처리는 그대로이며, 별도 실제 모델 품질 평가는 수행하지 않았다.
