# Codex 0.159.3 연동 업데이트와 업데이트 명령

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

설치 상태: `codex`와 `codex-maeklog` 모두 0.159.3 적용·실제 연동 검사 완료.

## 변경

공식 `rust-v0.159.3`의 기준 commit은
`01fc69f4026735edfdf6789820549727a4867b11`이다.
[공식 릴리스](https://github.com/openai/codex/releases/tag/rust-v0.159.3)와
[0.159.2 대비 변경](https://github.com/openai/codex/compare/rust-v0.159.2...rust-v0.159.3)을 확인했다.
이번 변경은 계정 보안 설정 알림과 workspace 버전이며, Maeklog에 필요한 터미널별
클라이언트 출처 계약은 추가되지 않았다. 기존 TUI bridge를 충돌 없이 적용했다.

패치: [codex-0.159.3-maeklog.patch](../integrations/codex/codex-0.159.3-maeklog.patch).
SHA-256: `4d842e126e1d9adcaefa3bb6bdbc8b002326a956ba715def8767c9a697dd3af7`.
새 버전에서도 활성 대화·입력·전환·정상 종료의 연결을 터미널 클라이언트가
보고한다. 서버와 모델 프롬프트의 동작을 바꾸는 패치는 없다.

## 반복 업데이트 줄이기

[update-codex-maeklog.py](../scripts/update-codex-maeklog.py)를 만들고
`~/.local/bin/maeklog-update`에 연결했다.

```sh
maeklog-update --check
maeklog-update --apply
maeklog-update 0.159.3 --apply
```

공식 stable 버전 조회 → 소스 다운로드 → 패치 호환 확인 → 의존성·빌드 →
일치하는 공식 helper 준비 → 연결·실제 lifecycle 검사 → 백업·설치를 수행한다.
적용 전 검사에서 실패하면 현재 실행 링크를 교체하지 않는다. 이미 같은 버전이
두 실행 링크에 연결되고 설치 manifest의 실행 파일 hash가 모두 같으면 빌드를
생략한다. 동시에 실행하는 업데이트는 차단한다.

공유 서버가 물려준 pane 환경을 업데이트 명령이 그대로 믿지 않도록 했다.
Codex tool에서 실행하면 현재 thread ID와 live Herdr 상태로 호출 pane을 찾는다.
일반 셸에서 실행하면 그 셸의 caller context를 쓴다. 실제 호출 pane 선택과
`--check` 동작을 확인했다.

명령의 자동 검사 범위는 bridge 단위 검사, TUI lint, 설치기, Maeklog routing과 실제
Herdr lifecycle이다. 전체 upstream TUI suite를 자동으로 통과시킨다는 의미는
아니다. 아래 전체 검사와 공식 원본 대조는 이번 수동 이식에서 별도로 수행했다.
자동 명령의 단계별 중단·설치 호출 순서는 모의 검사로 검증했고, 이번 업데이트의
빌드·설치 구성 요소는 각각 실제 실행으로 검증했다.

소스 패치와 컴파일 자체는 계속 필요하다. 향후 충돌이 생긴 버전은 수동 이식이
필요하며, 공식 클라이언트 출처 계약이 생기면 이 소스 의존을 제거할 수 있다.

## 수행한 검사

- `just fmt` 및 최종 formatter: 통과.
- Codex TUI 전체: 5,575개 실행, 5,536개 통과, 화면 snapshot 38개 실패,
  1개 시간 초과, 8개 skip. 전체 통과로 표시하지 않는다.
- Herdr bridge의 attach·대화 전환·detach 순서 검사: 통과.
- 실패 38개를 정확한 공식 0.159.3 원본 TUI로 다시 실행: 같은 38개 실패.
  수정본에만 남는 실패는 0개. 원본 파일을 HEAD와 byte 단위로 대조한 뒤 검사했다.
  30개 snapshot은 버전 문자열만 바꾸면 같고, 나머지 8개는 버전 길이에 따른
  정렬·줄바꿈과 서버 버전 안내 표시가 함께 달랐다. 개발 버전 `0.0.0`의
  기대값과 release 버전의 차이를 확인했으며 새 snapshot을 승인하지 않았다.
- 시간 초과한 `unsupported_workspace_plans_hide_top_chats_and_skip_loading`:
  수정본에서 단독 실행하여 통과, 53.837초.
- `just fix -p codex-tui`: 통과, 4분 09초. 공식 core의 기존 unused import
  warning은 그대로다. 이후 formatter와 패치 역적용 검사를 통해 파일을 확인했다.
- Cargo.lock: workspace 159개 package의 `0.0.0` 버전만 `0.159.3`으로 정규화.
  외부 package와 나머지 의존 그래프는 원본과 같음을 코드로 확인했다.
- `just bazel-lock-update`: 통과, `MODULE.bazel.lock` 변경 없음.
  설치된 Bazelisk cache의 해당 버전 Bazel을 사용했다.
- 설치된 Maeklog hook의 `test-client-routing.mjs`: 통과. 지연된 화면 인식,
  실제 PID·프로세스 이름, 공유 서버의 잘못된 환경, 같은 cwd의 탭 분리,
  전환·늦은 답변·오래된 detach·중복 viewer·pane 이동·opt-out 등을 확인했다.
- 격리 HOME의 `test-install-client-routing.py`: 통과. 버전 불일치 거부,
  helper 포장, 두 링크, 백업, 설정·기록 보존과 중복 설치 거부를 확인했다.
- `test-update-codex-maeklog.py`: 8개 통과. 패치·검사·빌드·routing·live 검사
  실패 시 설치 호출이 없고, 호출 pane 선택·의존성·설치 hash 확인이 동작한다.
  npm이 platform package를 최상위 또는 하위에 설치한 경우 모두 검사했다.
- 실제 `maeklog-update --check`: 공식 최신 stable `0.159.3` 확인.
- 최적화 release 빌드: 통과, 19분 13초. 기존 방식대로 최종 CLI crate에만
  `-C lto=off`를 사용했다. 공식 app-server/cloud-tasks의 기존 warning은 남겼다.
- 실제 `maeklog-update 0.159.3 --apply`: 설치 hash를 확인하고 재빌드 없이 종료.

검증 자료는
`/path/to/private/validation-artifact`에 보관한다.
실제 사용자 대화나 인증 정보를 fixture에 복사하지 않는다.

## 적용 결과

두 가지 실제 Herdr 검사를 통과했다.

- 후보 0.159.3 클라이언트 + 공식 0.159.2 서버: `live-candidate-old-server/result.json`.
  기존 공유 서버를 유지할 때의 클라이언트 연결·수집을 확인했다.
- 설치된 `~/.local/bin/codex` + 별도 공식 0.159.3 서버:
  `live-installed/result.json`. 설치 위치와 기본 helper 경로를 확인했다.

두 검사 모두 시작 시 첫 입력 전 등록, 실제 요청·최종 답변 수집, `/new`의
등록 전환, 정상 종료의 detach, 저장된 대화 resume와 후속 턴 수집을 통과했다.
서버에 의도적으로 잘못된 pane/tab 환경을 줬으며 수집 결과는 실제 터미널의
탭에 귀속됐다. 각각 요청 2개와 최종 답변 2개, 총 4개 이벤트가 저장됐다.
fixture 응답은 localhost가 제공했고 외부 모델은 호출하지 않았다.
검사에서 만든 pane과 별도 서버만 종료했다.

- 설치 package: `$HOME/.local/share/maeklog/codex-0.159.3-maeklog.1`.
- `~/.local/bin/codex`와 `~/.local/bin/codex-maeklog`가 같은 새 package를 가리킨다.
  두 명령 모두 `codex-cli 0.159.3`을 출력한다.
- Codex SHA-256: `ba29f4c083c217648c01b78c99c2999888077dbb6e4a3c1d3a8ce54de0962845`.
- 공식 code-mode helper SHA-256:
  `fe64bd08c7844ca1fca91d8cb825e62012a03a05ae8040c635bbb61e5be0b4c5`.
- Maeklog hook SHA-256은 기존과 같은
  `056d83632dbbd286bacf0d04c02a01b556f020fcd23bb3c47cd0e6ce32714d63`.
- 백업: `/path/to/private/installation-backup`.
  `restore-manifest.json`에 이전 실행 링크와 hook/config 백업을 기록했다.
- `config.toml`, `hooks.json`, Maeklog main, Maeklog hook, 기존 글로벌 npm package.json의
  내용 hash가 설치 전후 같다. 새 package manifest의 실행 파일 hash 4개도 대조했다.
  기존 routing registry를 초기화하지 않았고, 과거 대화 복원이나 재요약을 요청하지 않았다.
  사용자 CLI·공유 daemon·기존 Maeklog 패널을 재시작하지 않았다.

이미 열린 Codex는 기존 코드를 계속 사용한다. 작업을 마친 뒤 정상 종료하고
같은 Herdr 터미널에서 `codex resume`으로 재개해야 새 CLI가 적용된다.
