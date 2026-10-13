# 결정 근거 계약 v4 — v0.12.3 개발 검증

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

## 재현과 원인

9월 15일 demo-space의 진단 자료에서 `proposal/question is not a confirmed decision`을 확인했다. 당시 인용은 원문에 정확히 존재했고 모델 입력과 원문도 같았다. 모델은 누락 지적을 결정으로 작성했으나 코드는 모든 사용자 근거에서 `?`/‘할까요’/‘제안’/‘검토 중’ 포함 여부만으로 거절했다. 이 두 기준의 충돌이 확인된 사례다. 모든 과거 실패의 원인이 같다는 뜻은 아니다.

이번 작업 시점의 최신 단일 진단 파일은 후속 `quote_not_in_event` 실패로 교체돼 있었다. 따라서 이전 패킷 전체를 재생했다고 주장하지 않는다. 앞서 확인한 누락 질문 형태의 축소 사례와 합성 명시 승인 사례를 구분해서 검사했다. 전체 사적 진단 파일을 저장소에 복제하지 않았다.

먼저 `explicit_confirmation_is_not_rejected_because_support_contains_a_question` 테스트를 추가했다. ‘버튼은 어디갔어? 시점 선택 버튼을 유지하기로 확정한다.’에서 확정 문장을 첫 근거, 질문을 보조 근거로 주었는데 기존 코드에서 실패했다. 수정 후 같은 검사가 통과한다. 이것은 문장부호의 기계적 거절이라는 코드 결함의 비모델 재현이며 모델 분류 품질 평가가 아니다.

## 구현

`decision_policy.rs`가 모델 안내와 코드의 결정 수용 계약을 묶는다. schema/context-v4.schema.json에 `decision_basis`를 추가했다. 현재 설치본이 읽는 v3 스키마는 변경하지 않았다.

결정 근거에는 `intent`, `reference`, `quote`가 있다. explicit_choice / explicit_approval / conditional_approval만 수용하고 request / question / proposal / uncertain은 확정 결정으로 저장하지 않는다. 일반 질문·복원 요청을 자동으로 결정으로 승격하지 말고 필요한 경우 state_updates에 작성하도록 지시한다. 코드가 임의로 실패 항목을 상태로 변환하지는 않는다.

별도 확정/승인 구절은 첫 근거와 같은 사용자 사건을 참조하고 그 인용의 정확한 부분 문자열이어야 한다. 조건부 승인은 condition이 비어 있으면 거절한다. 보조 근거나 문장 안의 물음표만으로 승인 구절을 부정하지 않는다. completion에는 decision_basis가 없어야 한다. 원문 인용 일치, 사용자 anchor, 변경 관계, 사건 시각 검사는 유지한다.

새 결정 버전에 basis를 저장한다. 구버전 Version은 basis가 없어도 읽으며 추정·일괄 이관·자동 삭제하지 않는다. 기존 결정·완료 보존, 묶음 단위 커밋과 중복 방지는 유지한다. 모델이 질문을 explicit_choice로 잘못 분류하고 그 질문 자체를 승인 구절로 붙이면 코드만으로 의미를 보증할 수 없다. 새 구조는 독립적인 승인 판정기나 근거 ID 방식이 아니다.

## 재시도

RetryState에 기존 파일과 호환되는 validation_error / repeated_validation 필드를 추가했다. 최종 응답 검증 오류를 500자까지 같은 `daily:today|manual:<terminal-seq>` 작업의 기존 재시도에 진단 데이터로 전달한다. 입력이 처리 중 새로 유입돼도 피드백 키는 현재 선택한 사건에서 계산하며 최신 대기열을 다시 선택하지 않는다. 원래 응답 전문/사적 진단 파일은 재시도에 보내지 않는다.

같은 제한된 오류 문자열이 두 번 연속이면 attempts=2 상태로 자동 중단한다. 세 번 실패했다고 거짓 표시하지 않고 동일 검증 오류 반복과 실제 오류를 표시한다. 서로 다른 오류나 프로세스 실패는 기존 최대 3회 규칙을 유지한다. 신규 작업은 다른 키이므로 피드백을 상속하지 않는다. 명시적 r/b의 기존 초기화 의미는 그대로다. 새 이벤트에 같은 잘못된 모델 판단이 반복되는 문제까지 전역적으로 차단하는 것은 아니며, 전체 Space를 자동 정지시키지 않는다.

## 검증

- 기존 코드에서 위 회귀 테스트 실패를 먼저 확인했다.
- 단위 테스트 103개 통과. 명시 확정+질문 보조 근거, 질문/요구/제안/미확정의 거절, 상태로만 반영한 요청, 조건부 승인, 기존 기록의 비파괴 읽기, 원문 인용 위조 거절, 반복 오류 중단/직렬화/키 분리를 검사했다.
- fmt 및 clippy `-D warnings` 통과, 격리 release 빌드 통과.
- 비모델 통합 8개 통과: decision-contract, reducer-diagnostics, branch-memory, daily-policy, opt-out, recovery, parallel, panel-menu. 새 모의 통합은 누락 질문의 basis 없는 결정 거절 → 기존 재시도의 오류 전달 → 수정된 mock의 상태 저장, 질문이 함께 있는 명시 확정 수용, 동일 검증 오류 두 번 후 세 번째 모델 호출 없음, 다른 작업 피드백 없음, v4/high 인자를 확인한다.
- 실제 모델 호출/품질 평가, 속도 비교는 하지 않았다. 테스트는 의도한 분류를 mock에 넣는 구조이므로 Luna가 실제로 그 분류를 선택한다는 증거가 아니다.
- 개발 단계에는 cargo-audit 실행 파일이 없어 검사가 보류됐으나, 아래 적용 단계에서 cargo-audit 0.22.2를 임시 경로에 설치하고 새로 조회한 RustSec DB로 `audit --deny warnings`를 실행해 통과했다(2026-09-15, advisories 1246개, 의존성 36개, exit 0). 의존성 변경 없음.

검증 빌드: `/path/to/private/validation-artifact`

SHA-256: `dce96e0debbbc0d91377c60468337e13cf71cc13a852b330ea721d3a7ed064ed`

## 적용 경계와 다음 단계

개발 완료 시점에는 설치 main/hook을 v0.12.2 그대로 유지했다. 이후 사용자가 현재 실행 중 패널 적용을 명시 승인하여 아래와 같이 교체했다. 실제 모델 평가, 훅 재등록, 원격 변경·병합·배포는 하지 않았다. Luna high / 180초 / 동시 3 Space / 수집 범위 / 오늘 자동·이전 날짜 수동 정책도 유지한다.

새 Version 필드의 보존을 위해 이전 바이너리의 원장 쓰기 작업이 끝난 뒤 교체해야 한다. 구버전 worker와 신버전 worker를 섞어 새 basis를 잃게 만들면 안 된다. 실제 모델 평가는 별도 승인받아 질문·누락 지적·명시적 요청·확정·조건부 승인의 분류를 확인해야 한다. 이번 검증 결과만으로 모든 실제 요약 실패가 해결됐다고 말하지 않는다.

## 2026-09-15 실행 중 패널 적용

- 첫 교체 시도는 작업 잠금 획득 실패로 바이너리 교체 전에 중단됐다. 재시도에서 coordinator 및 모든 기존 Space 작업 잠금을 확보했고 구버전 worker-space가 없음을 확인한 뒤 교체했다. 실행 중 요약을 강제 종료하지 않았다.
- 검증 빌드를 main/hook 경로에 각각 원자적으로 교체했다. 열려 있던 Maeklog 패널만 hide/open으로 다시 열었다: demo-shop `<runtime-id-15>`, demo-space `<runtime-id-16>`. 수집 disable/enable이나 훅 재등록을 사용하지 않았다.
- main/hook 및 실제 두 패널 프로세스 실행 파일 해시가 위 검증 빌드와 일치한다.
- 잠금 상태에서 비교한 기존 브랜치 journal 해시와 Space별 enabled/generation, 날짜 보류·수동 요청·재시도 파일 해시가 적용 전후 동일하다. Codex hooks.json과 hook-runtime.json 해시도 동일하다.
- 기존 대기열을 삭제하거나 전체 과거를 재요약하지 않았다. 재시도 초기화나 수동 날짜 요청도 하지 않았다. 이후 정상 자동 처리에는 새 버전이 사용된다. 실제 모델 품질·속도 개선은 이번 적용 확인만으로 검증되지 않았다.

OpenAI Docs에서 [구조화 출력도 의미상 오류가 남을 수 있음](https://developers.openai.com/api/docs/guides/structured-outputs#handling-mistakes)을 확인하고, 출력 구조 검증과 모델의 의미 판단 품질을 구분했다.
