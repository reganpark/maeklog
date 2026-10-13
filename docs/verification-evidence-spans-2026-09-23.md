# 원문 구간 선택으로 인용 불일치 방지

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

2026-09-23 실제 실패 진단을 원문과 대조했다. demo-space에서는 줄바꿈을 합치거나 Markdown 링크를 표시 문구로 바꾼 인용, business에서는 강조 기호를 제거한 인용이 exact substring 검증에서 거절됐다. 두 진단 모두 모델 입력 이벤트와 원본 이벤트는 동일했다. 사적 대화 원문은 저장소에 복제하지 않았다.

새 모델 출력 스키마 v5는 journal evidence와 decision_basis의 자유 인용문 대신 사건 reference와 1부터 시작하는 segment_start/segment_end를 요구한다. 입력에 사건별 원문 구간 목록을 제공한다. 구간은 줄바꿈을 포함해 보존하고 긴 줄은 유니코드 문자 240개 단위로 나눈다. 선택한 연속 구간을 코드가 합쳐 기존 quote 필드로 변환한 후 기존 원문 일치, 결정 의도, 사용자 근거, 조건, 원장 검증을 모두 수행한다.

없는 사건, 범위 밖·역순·불완전한 선택, quote와 선택자의 혼용, 공백뿐인 구간, 600자를 넘는 인용은 저장하지 않는다. 기존 저장 형식과 기록은 변환하지 않는다. 과거 quote 응답을 사용하는 계약 fixture는 기존 exact 검증으로 계속 검사하지만 새 모델 출력 스키마에는 quote 필드가 없다. 실패 진단에는 원래 선택자 응답과 이벤트가 남아 구간 목록을 재구성할 수 있다.

모델이 잘못된 구간이나 의미를 선택하는 오류까지 제거하는 것은 아니다. 전체 묶음의 원자적 검증과 재시도 한도는 유지하며 추가 보정 모델 호출은 없다. 입력에 구간 목록이 추가되므로 모델 입력량은 증가한다.

검증: 단위 112개와 clippy 통과. 강조·링크·줄바꿈의 원문 보존, Unicode 경계, 범위 오류, 길이 제한을 검사했다. release 빌드, fmt, diff 검사와 비모델 통합 reducer-diagnostics/decision-contract/branch-memory/daily-policy 4개도 통과했다. 통합 검사에서 잘못된 선택자 응답의 진단 저장·미커밋 및 Markdown/줄바꿈을 포함한 정상 구간 선택의 저장을 확인했다. 실제 모델 품질 평가는 수행하지 않았다.

로컬 main 및 hook 실행 파일에 적용했다. SHA-256: `096a0017a2316e84b871d0eb7ce46c5295e6c0a78a7ce84d7ca37078f633e2e2`. hook은 교체 전 백업을 만들고 원자적으로 교체했다. 실행 중인 표시 패널은 재시작하지 않았다. 이후 새 hook에서 시작하는 요약에 적용되며 이미 재시도 한도를 소진한 묶음은 자동 재개하지 않는다. 수집 설정, hooks.json, 원장, 대기열, 재시도 상태는 변경하지 않았다.

후속 적용 오류: 기존 패널에서 재시도 시 `No such file or directory (os error 2)`가 발생했다. 실행 파일 교체 후 Linux `/proc/PID/exe`가 이전 바이너리에 `(deleted)`를 붙인 경로를 반환했고, `worker::spawn_detached`가 `current_exe()` 경로를 실행하려다 실패하는 경로를 확인했다. 패널을 재시작하지 않은 적용 누락이다. 원장/원문 손실을 뜻하지 않는다. 이 커밋은 실행 중 바이너리 교체를 자동 복구하는 변경을 포함하지 않는다.

로컬 커밋 직전 확인: 열린 패널 6개 중 3개는 새 release 해시와 일치하고, 3개는 여전히 삭제된 이전 실행 파일을 참조한다. 새로 열린 패널에는 적용됐으나 나머지 패널은 닫고 다시 열어야 한다. 모든 패널 적용 완료 또는 실제 요약 성공 확인으로 보고하지 않는다.
