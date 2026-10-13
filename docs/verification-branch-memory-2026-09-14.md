# Maeklog v0.11.0 개발·검증 기록

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

검증일: 2026-09-14. 환경: Linux, rustc 1.97.1, Node.js v24.18.0, Python 3.

## 결과와 권한 경계

로컬 구현·비모델 테스트·문서 갱신을 완료했다. 사용자 요청에 따라 실행 중인 플러그인 교체, 훅 재설치, 실제 모델 평가, 원격 변경·병합·배포는 하지 않았다. 이 결과는 배포 승인이나 실제 모델의 의미 판단 품질 보증이 아니다.

저장소와 상위 경로에 적용할 AGENTS.md가 없음을 확인했고, 기존 README·코드·검증/보안 절차를 읽은 뒤 [짧은 계획](implementation-plan-2026-09-14.md)을 설명하고 구현했다. Herdr 스킬의 제어 경계와 Impeccable harden 지침을 사용했다. 기존 터미널 스타일을 유지하면서 한글 줄바꿈, 실패·미확인 구분, 키보드 접근과 오프라인 캐시 동작에 적용했다. 새 디자인 시스템이나 앱 대시보드는 만들지 않았다.

초기 작업트리는 파일 전체가 untracked인 상태였다. 기존 자료를 보존했으며 staging·commit·원격 쓰기는 하지 않았다. 실제 사용자 Space나 Maeklog 기록을 삭제하지 않았다. 삭제 테스트는 임시 fixture만 대상으로 했다.

## 구현 요약

- 저장 단위: 정규화된 Git common directory + 브랜치명 + 보수적인 reflog 세대 식별. 같은 브랜치/worktree의 Space는 공유하고 참여 설정은 Space별로 유지한다.
- 결정·완료: 안정적인 ID/의미 키/단계/출처·인용·버전·변경/후속 관계를 가진 누적 JSON 원장이다. 생략·재요약·세션/브랜치 이동·끄기·구현 완료로 삭제하지 않는다.
- 시각: 첫 근거 이벤트의 원본 UTC 시각만 사용한다. 추가 근거/현재 시각/mtime으로 미확인 시각을 채우지 않는다. 동일 상태/재진술은 기존 시각을 유지하고 실질적 변경에만 근거 시각을 새로 연결한다. 로컬 시간대와 연도·오프셋을 표시한다.
- 재처리: 동일 source ID는 원장 처리 지문으로 고정한다. 같은 이벤트를 다른 키/인용으로 재요약해도 새 사실을 만들지 않는다. 최초 한 이벤트의 여러 사실/단계는 한 배치에서 모두 받을 수 있다.
- 현재 상태: 진행·막힘·주체/조건을 포함한 다음 행동을 별도 저장한다. 막힘 생략은 해제가 아니라 미확인이다. 뒤늦은 과거 결과는 최신 상태·결정 변경 관계를 덮어쓰지 않는다.
- 패널: 현재 결정 전부, 최근 완료 3건, 나머지는 h로 접기/펼치기. [ / ]로 25개 기록 묶음 이동. 실패가 이전 완료로 밀려도 기본 경고를 보인다. h/페이지는 순수 표시 동작이며 resize/refresh로 초기화되지 않는다.
- GitHub: 정확한 저장소/번호의 수동 주 연결, 여러 후보, PR 미생성 명시, 명시적 읽기 전용 refresh와 실패 캐시. PR 설명 발췌는 diff 검증으로 표시하지 않는다.
- 이관: 구버전 current/timeline 원본을 비파괴 보관하며 개별 확정 사실로 승격하지 않는다. 미연결 수집 항목과 구버전 스냅샷은 h 및 memory CLI로 접근한다.
- 삭제: Space clear는 사적·미연결 자료만 지우며 공유 기여는 남긴다. 공유 원장/링크 삭제는 exact scope key 반복 확인, 관련 Space 중지·대기열 비움이 필요하다. 원본 Codex 대화·별도 Space 스냅샷은 별개다. 중복 방지 지문/수집 순서/꺼짐·범위 식별자는 남긴다. 자체 undo는 없다.
- 기존 제한: 고정 Luna Max, 수집 역할, 동시 3 Space, Space당 단일 처리, 기존 배치/재시도/timeout, 최근 완료 최대 3턴 재활성화를 유지했다. 전체 원장을 모델 입력으로 보내지 않고 제한된 부분 색인만 사용한다.

## 실제 패널 출력

모의 대화와 fixture/demo-shop의 모의 GitHub 메타데이터를 실제 release 바이너리에 넣은 격리 PTY 실행 결과다. 사용 중인 Herdr pane에 새 바이너리를 설치해 찍은 화면은 아니다.

| 상태 | 산출물 |
| --- | --- |
| 기본 접힘 | [PNG](captures/branch-memory/panel-default.png), [52열 출력](captures/branch-memory/panel-default.txt), [실제 PTY ANSI](captures/branch-memory/panel-default.ansi) |
| h로 펼침 | [PNG](captures/branch-memory/panel-history.png), [52열 출력](captures/branch-memory/panel-history.txt), [실제 PTY ANSI](captures/branch-memory/panel-history.ansi) |

[재현 방법](captures/branch-memory/README.md). PNG는 ANSI 좌표/문자를 그대로 래스터화한 보기용 이미지다. 사용자 터미널의 폰트·테마와 픽셀 단위로 동일하다고 주장하지 않는다.

## 실행한 검사

격리 target: `/path/to/private/validation-artifact`. 최종 release SHA-256:

`0fd56a966f12dab28fddff43b36003a4f4c8f429de1bb3bfc621fbc4e42851d1`

| 검사 | 최종 결과 |
| --- | --- |
| cargo fmt --all --check | 통과 |
| cargo clippy --locked --all-targets --target-dir /path/to/private/validation-artifact -- -D warnings | 통과 |
| cargo test --locked --target-dir /path/to/private/validation-artifact | **84 passed, 0 failed** |
| cargo build --locked --release --target-dir /path/to/private/validation-artifact | 통과 |
| test-recovery.mjs (격리 release) | 배치·대기 원문·기존 요약·재시도·세션 연결·기존 검증 규칙 통과 |
| test-latest.mjs (격리 release) | 최신 우선/시간순 분리·독립 재시도·재시작·불필요한 중복 호출 방지 통과 |
| test-parallel.mjs (격리 release) | 동시 3 Space·4번째 슬롯 차단·장애 격리·coordinator 재시작·새 입력 보존 통과 |
| test-opt-out.mjs (격리 release) | incremental/latest 두 경우 모두 중단·다른 Space 격리·최근 3턴·과거 전체 차단·명시적 삭제 통과 |
| test-panel-menu.mjs (격리 release) | 실제 PTY 우클릭 취소/숨기기/끄기 3경로, 입력 라우팅과 마우스 복원 통과 |
| test-branch-memory.mjs (격리 release) | v3 모의 reducer·GitHub 캐시/실패/다중 후보·실제 h/scroll/80→30→80 resize·표시 동작 무변경 통과 |
| git diff --check | 통과. 단, 기존 untracked 파일의 전체 diff 검증을 뜻하지 않음 |
| schema JSON 읽기 및 문서 상대 링크 | 파싱/링크 존재 확인 |

모델 호출 경로는 테스트 실행 파일로 대체했다. 테스트 프로세스에 고정 모델 인자가 전달되는 것까지 검사했지만 실제 모델 서비스를 호출하지 않았다. GitHub도 별도 mock executable로 대체했다.

중간 실패는 숨기지 않고 수정 후 재검증했다. 기존 동시 append 테스트가 동일 turn ID/내용을 50번 재사용하던 fixture는 ‘50개의 서로 다른 이벤트’라는 본래 의도대로 고유 turn ID를 부여했다. 단계가 다른 완료가 하나로 중복 제거되던 연산 지문 문제를 고쳤다. 새 PTY 테스트에서 resize 이전 프레임을 읽던 대기 조건은 실제 좁은/넓은 footer와 스크롤 결과를 확인하도록 수정했다. 최종 표는 수정된 최종 소스/테스트의 결과다.

## 요청한 9가지 검증과 대응

| 기준 | 근거 |
| --- | --- |
| 1. 새 요약/재시작/브랜치 이동 뒤 결정·완료 보존 | branch_tests의 누적 보존·checkout·재활성화 테스트 |
| 2. 제안 제외, 변경·철회·실패 후속 연결 | 제안/조작된 인용 거부, 늦은 결정 변경/철회, 실패→재검증 연결 테스트 |
| 3. 동일 이벤트/재시도 중복 방지, 원래 시각 유지 | 단계별 구분, 다른 키/인용 재처리 무변경, restate·현재 상태 반복 시각 테스트 |
| 4. 접기·건수·스크롤·좁은 한글·무호출 | 31개 완료/이전 28개/두 페이지, 12·18·24·40·52열 단위 검사, 실제 PTY h/End/Home/resize, 원장 bytes·mock 호출 수 동일 |
| 5. 브랜치 전환·동시 Space·늦은 결과 | 실제 임시 Git/worktree, 턴 중 checkout, checkout 후 동일 이벤트 재전달, 두 Space 동시 원장 병합, 과거 상태/결정 결과 검사 |
| 6. Space 끄기·최근 3턴·공백 | 기존 opt-out 두 모드 통합, restart/transcript/participation 단위 검사와 gap 보존 |
| 7. 원본 시각 없음 | legacy 시각 미확인, user anchor unknown + support known 검사, 로컬 TZ/DST 표시 검사 |
| 8. 이슈·PR 미연결/다중/실패 | 번호만 입력 거부, 주 연결 지정, PR 미생성, 여러 후보, 정확한 OPEN/head 일치, 실패 시 캐시/시각 보존 |
| 9. 비파괴 이관/삭제 | 구버전 원본 보존, 미연결 항목 열람, Space 삭제가 공유 원장을 남기는지, 켜짐/대기 입력의 공유 삭제 거부, 명시적 공유 삭제 검사 |

## 실행하지 않은 검증과 남은 제한

- 실제 Luna 의미 판단/출력 품질 평가: **미실행**. 사용자가 금지한 실제 모델 평가에 해당한다. 인용·역할·관계 구조의 검증은 의미가 참이라는 증명이 아니다.
- 사용 중인 Herdr pane 교체 후 실기 확인, macOS/BSD date 실기·OS 재부팅: **미실행**. 격리 Linux PTY/coordinator 재시작 테스트와 구별한다.
- 실제 GitHub 인증·네트워크·서비스 장애: **미실행**. 모의 응답과 실패 종료를 검사했다.
- RustSec audit: **미실행**. cargo-audit가 없어 버전 확인이 실패했다. 의존성 추가는 없지만 전체 release 보안 gate가 완료됐다고 주장하지 않는다.
- 브랜치 이름 변경·재생성·reflog 교체를 자동 이어 붙이거나 별도 clone을 합치는 기능은 없다. 미연결/별도 범위로 보존한다.
- 안정적인 turn ID가 없는 다른 Space의 이벤트, 새로운 근거·다른 의미 키로 표현된 같은 뜻의 사실은 완전한 의미 중복 판정을 보장하지 않는다.
- 처음 처리에서 누락된 의미를 이미 처리한 동일 이벤트 재요약으로 자동 보충하지 않는다. 이는 재시도 결과가 누적 사실을 증식시키지 않도록 한 경계다.
- 대규모 장기 원장 성능·임의 전원 차단의 모든 파일 쓰기 지점·물리 터미널/접근성 환경은 검증하지 않았다. JSON 읽기/교체 비용은 기록 수에 비례한다.
- 기록 삭제는 자체 복구가 없고 최소 중복 방지 메타데이터는 남는다. 완전한 민감정보 제거에는 원본 Codex 대화·별도 스냅샷·백업의 별도 범위 검토가 필요하다.

## 실행 중 설치 보존 확인

작업 전후 다음 SHA-256이 동일했다.

| 파일 | SHA-256 |
| --- | --- |
| 저장소 target/release/maeklog | 671c3a56031aa87272b1bac4c6855b9adc75ea93bc4cd0487ce113e62b4f16d0 |
| 설치 bin/maeklog-hook | 671c3a56031aa87272b1bac4c6855b9adc75ea93bc4cd0487ce113e62b4f16d0 |
| $HOME/.codex/hooks.json | d042b423c5112b3e4031466088c1e4091e1b2afe4fce4518d7186c8ec19c66d0 |

새 실행 파일은 격리 target에만 있다. 이후 **실행 중 플러그인 교체, 훅 재설치, 실제 모델 평가, 원격 변경·병합·배포는 각각 별도 사용자 요청/승인이 필요**하다.
