# Codex CLI 0.157.1 공유 서버와 탭 귀속 오류

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

2026-09-28 로컬 설치, 실행 중 프로세스 환경, Herdr pane 정보와 Maeklog 저장 자료를 읽기 전용으로 조사했다. 사용자 대화 본문은 이 문서에 복사하지 않았다.

## 확인된 현상

- 설치된 CLI는 `codex-cli 0.157.1`이다. `codex features list`에서 `daemon_auto_start`가 `stable true`로 표시된다.
- 후속 확인: 별도 빈 `CODEX_HOME`에서 보관된 실행 파일의 `features list`만 실행해 비교했다. `0.156.0`과 `0.156.1`은 `daemon_auto_start experimental false`, `0.157.1`은 `stable true`다. 사용자 `config.toml`에는 이 설정의 명시적 재정의가 없다. [공식 변경 기록](https://learn.chatgpt.com/docs/changelog)은 2026-09-25 배포된 `0.157.0`에서 적용 가능한 대화형 세션의 백그라운드 서버 자동 시작을 활성화했다고 명시한다. 공유 서버 기능의 최초 도입과 기본 자동 시작 전환은 구분해야 한다.
- 공유 app-server 프로세스에는 최초 실행 위치의 `HERDR_WORKSPACE_ID=<runtime-id-1>`, `HERDR_TAB_ID=<runtime-id-2>`, `HERDR_PANE_ID=<runtime-id-3>`이 들어 있다. 해당 pane은 demo-business 탭이다.
- maeklog를 실행하는 CLI 클라이언트의 환경은 `<runtime-id-4>` / `<runtime-id-5>`이지만, 이 대화의 도구 프로세스가 상속한 환경은 `<runtime-id-2>` / `<runtime-id-3>`이다.
- `src/hook.rs`는 `herdr::caller_context(false)`를 호출한다. `src/herdr.rs`는 상속한 pane에 대한 `herdr pane current --current`를 우선한다. 공유 서버의 pane 정보가 다른 탭에도 재사용되므로 실제 클라이언트 탭을 구별하지 못한다.
- 훅의 `session_id`와 `cwd`는 대화별로 달라지지만 Herdr 탭 환경은 그대로다. 따라서 저장 자료에서도 서로 다른 대화의 탭 귀속이 잘못된 것을 확인했다. 이 문제는 세션 ID나 cwd 누락이 아니다.

## 저장 자료의 영향

조사 시점 demo-business 탭의 `event-sources.json`에 기록된 오늘 이벤트는 아래와 같다. 성공적으로 요약한 이벤트도 출처 자료에는 남으므로 현재 inbox 건수와는 다르다.

| 이벤트의 원래 cwd | 이벤트 수 | 저장된 탭 |
| --- | ---: | --- |
| `$HOME/repos/demo-business` | 10 | `<runtime-id-2>` |
| `$HOME/repos/demo-space` | 5 | `<runtime-id-2>` |
| `$HOME/repos` | 10 | `<runtime-id-2>` |
| `$HOME/repos/maeklog` | 1 | `<runtime-id-2>` |

demo-space 자료에는 잘못된 `<runtime-id-2>` 귀속으로 생성된 브랜치 원장도 있다. 저장소별 분리는 유지되지만, 올바른 탭에서 해당 기록을 볼 수 없다. maeklog의 실제 `<runtime-id-4>` 패널은 수집 기록이 없는 상태로 표시된다.

오늘 요약 성공 및 idle 상태가 존재하고, `codex exec --help`에서 기존 요약 실행 옵션이 모두 확인된다. 따라서 관찰된 주요 장애는 실행 옵션 삭제가 아니라 훅의 탭 귀속이다. 모든 요약의 품질이나 다른 탭의 개별 오류까지 정상이라고 판정한 것은 아니다.

## 우회 방법

설치된 CLI의 `codex --help` 및 `codex resume --help`는 `--no-daemon`을 지원하며, 이미 실행 중인 공유 서버가 있어도 사용하지 않는다고 명시한다.

Herdr 안에서 새 대화는 다음과 같이 실행한다.

```sh
codex --no-daemon
```

기존 대화는 해당 탭에서 CLI를 정상 종료한 뒤 세션 선택기를 이용해 재개한다.

```sh
codex --no-daemon resume
```

여러 탭을 사용하는 환경에서는 `--last`로 대화를 추정하지 않고 해당 대화를 직접 선택한다. 공유 서버를 재시작하는 것만으로는 어느 한 탭의 환경이 다시 공유될 수 있으므로 탭 분리 해결책이 아니다.

실행 중인 대화를 중단하거나 재시작하지 않았으므로 우회 후 실제 새 턴의 수집·요약까지는 검증하지 않았다. 재개 후에는 SessionStart/새 요청의 출처 탭과 완료 답변의 반영을 확인해야 한다.

## 후속 수정 시 주의점

cwd만으로 탭을 선택하면 같은 저장소를 연 여러 탭을 구분할 수 없다. 현재 Herdr의 세션 연결 역시 daemon 환경을 상속하는 SessionStart 훅에 의존하므로, 그 연결을 그대로 신뢰하는 자동 재귀속은 충분하지 않다. 공유 서버를 지원하려면 클라이언트별 출처가 전달되는 별도 계약이 필요하다.

기존 기록의 재귀속은 출처와 원장, 처리 지문을 함께 검토해야 한다. 이번 조사에서는 대기열·원장·참여 설정·훅 신뢰 설정을 수정하지 않았고, 과거 재요약이나 모델 평가 호출도 실행하지 않았다.
