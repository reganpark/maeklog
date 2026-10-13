# 실제 모델 요약 평가 — 2026-09-25

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

결론: v6 후보 선택은 이번 표본에서 정상 동작했으나, 요약 전체의 저장 안정성과 의미 정확성은 개선이 필요하다. 최초 저장 성공은 9/12(75%). 후보 선택은 33/33 유효했다. 저장된 결과 9개 중 3개에는 근거 없는 담당자 또는 조건 추가가 확인됐다. 작은 고정 표본이며 운영 전체의 성공률 추정치가 아니다.

## 방법

사용자의 실제 AI 평가 요청으로 운영과 동일한 release 바이너리·프롬프트·v6 스키마·검증·원장 저장 경로를 실행했다. 모델은 실제 `gpt-6-luna`, reasoning `medium`. 모의 모델을 사용하지 않았다. Herdr만 mock 처리했고 저장 공간은 전부 격리했다. 운영 원문 대신 실제 장애 형태를 반영한 합성 대화 6종을 각 2회 독립 실행했다. 호출 최대 동시성 3, 첫 시도만 평가했으며 재시도나 실패 재분류로 성공률을 높이지 않았다.

바이너리 SHA-256: `742d629821834ad262434585a82b96d04a0e2a76284f1d2fff2dbbbfcd88e79a`.

호출 12회 모두 종료됐으며 건당 8.7~21.9초, 평균 12.2초였다. 원래 출력은 worker가 삭제하기 전에 투명 wrapper로 별도 보존했다. 입력·출력·호출 인자·평가 기준·격리 원장은 `/path/to/private/validation-artifact`에 있다. 재현 실행 코드는 `/path/to/private/validation-artifact`. 임시 경로는 영구 보관을 보장하지 않는다.

## 결과

| 시나리오 | 최초 저장 | 내용 검토 |
|---|---:|---|
| 강조·링크가 포함된 캡션 완료 | 2/2 | 완료 내용 보존, 미완료 스킬 개선을 완료로 승격하지 않음 |
| 45번째 줄이 빈 줄인 카드 제작 보고 | 1/2 | 후보 선택은 둘 다 정상. 한 번은 key를 한글로 만들어 저장 거절 |
| 600자 초과 배경 문단 + 맞춤법 검토 | 2/2 | 맞춤법 검토 완료와 남은 사실관계 검증 구분 |
| 버튼 누락 질문·확인 요청 | 2/2 | 확정 결정·완료를 만들지는 않았지만 두 번 모두 담당자를 근거 없이 luna로 지정 |
| 검증 통과·별도 승인을 요구하는 배포 정책 결정 | 0/2 | 조건 문구는 유지. 두 번 모두 decision의 stage를 deployment로 출력해 저장 거절 |
| 단위 테스트 통과·미검증 브라우저·미배포 | 2/2 | 테스트 결과는 reported로 유지. 두 번째 실행은 원문에 없는 ‘브라우저 검증 후’ 배포 조건/순서를 추가 |

모든 응답의 journal evidence 및 decision_basis 33건을 입력 후보 목록과 대조했다. ID가 실제로 존재하고, 선택 결과가 공백뿐이지 않으며 600자 이하였다. 자유 quote·구형 범위 선택은 없었다. 이전 빈 줄·길이 초과 선택 오류는 이번 12회에서 0건이었다.

## 확인된 문제

1. **출력 스키마와 코드의 key 제약 불일치.** `blank45-1`이 한글 key를 출력했다. JSON 스키마가 이를 허용하지만 `journal::valid_key`는 ASCII 영숫자와 `._:-`만 허용한다. 결과는 `invalid durable record`. 후보 선택 실패가 아니다. 이 응답은 완료 근거의 첫 항목에도 사용자 요청을 배치해, 완료 보고가 시점 기준 근거가 되어야 한다는 요구를 지키지 못했다.
2. **결정의 주제와 기록 단계를 혼동.** `conditional-1/2`는 배포 정책 결정의 stage를 deployment로 출력했다. 코드는 decision kind에 decision stage를 요구한다. 오류 메시지 `decision needs confirmed user evidence`가 표시되지만 사용자 근거 자체는 존재했다. 문구만으로 실제 실패 조건을 알기 어렵다.
3. **담당자 추정이 저장됨.** `question_request-1/2`의 원문에는 Luna가 담당자라는 근거가 없는데 owner=luna로 저장됐다. 모델은 읽기 전용 요약자이며 작업 담당자로 지정된 적이 없다.
4. **원문에 없는 조건이 저장됨.** `reported_scope-2`는 ‘브라우저 검증과 배포를 아직 하지 않았다; 다음 행동은 브라우저 검증’이라는 입력에서 ‘브라우저 검증 후 운영 배포’라는 조건을 추가했다. 타당해 보이는 순서라도 원문 근거가 없으므로 정확한 기억으로 취급할 수 없다.

따라서 ‘근거가 원문에 존재한다’는 검증만으로 요약 문장의 모든 속성이 원문에 뒷받침된다고 보장할 수 없다. 저장 성공률과 의미 정확도를 분리해야 한다.

## 권장 후속 수정

- key의 허용 문자·길이를 모델 출력 스키마에 명시하고 코드 검증과 일치시킨다.
- 결정과 완료를 서로 다른 출력 구조로 구분해 결정의 stage/outcome 조합을 제한한다.
- 완료의 첫 근거는 완료 보고 사건을 가리키도록 검증·프롬프트를 보강한다.
- 담당자·조건은 명시된 근거가 없으면 unknown/null을 유지하고, 근거에 없는 일반적 작업 순서를 추가하지 않도록 평가 항목을 고정한다.
- 구조 오류 메시지를 원인별로 분리한다.
- 같은 사례 반복뿐 아니라 다양한 표현과 연속 대화에서 재평가한다.

이번 작업은 평가와 보고서 작성이며 위 후속 수정은 구현하지 않았다. 운영 원장·대기열·설정·실행 파일을 변경하거나 실제 운영 기록을 재요약하지 않았다.
