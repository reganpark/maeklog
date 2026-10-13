# Codex 0.159.2 연동 업데이트

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

## 원인과 변경

공식 npm 업데이트로 `~/.local/bin/codex`가 공식 0.159.2를 가리키면서,
기존 로컬 TUI의 터미널 클라이언트 등록 기능이 빠졌다. 설치된 Maeklog은
`client-routing.json`이 있는 경우 등록되지 않은 대화를 거부하므로,
공식 CLI로 새로 연 대화의 요청·답변을 수집하지 않았다.

공식 `rust-v0.159.2`의 기준 commit은
`ff6aec96948b70d94983af2641a6b67c94faeff5`다. 이 버전의 훅 입력도 실제
클라이언트 PID·pane 또는 이에 준하는 턴 출처를 제공하지 않는다. `client`는
클라이언트 애플리케이션 이름이며 터미널별 식별자가 아니다.
따라서 기존 TUI adapter를 새 버전의 대화 활성화·사용자 입력·종료 경로에 이식했다.
패치 파일: [codex-0.159.2-maeklog.patch](../integrations/codex/codex-0.159.2-maeklog.patch).

실제 재개 검증에서는 첫 TUI callback이 Herdr의 화면 기반 agent 인식보다
먼저 도착하는 경합도 확인했다. Maeklog의 연결 확인은 화면의 `agent` 표시를 필수
조건으로 삼지 않고, 동일 terminal ID에서 실제 전경 프로세스의 PID와 이름
`codex`가 모두 일치하는지 확인하도록 보완했다. 셸·교체된 프로세스·종료된
터미널은 여전히 거부하며, native 세션 ID 검증도 유지한다.

설치기는 버전과 upstream commit을 명시적으로 받도록 변경했다. 바이너리와
공식 resource package 버전이 다르면 설치를 거부한다. 두 진입점 `codex`와
`codex-maeklog`를 함께 갱신하고 이전 링크를 백업한다. 0.159.2의 별도
`codex-code-mode-host`를 포함해 공식 helper 실행 파일도 패키지에 보존한다.

## 자동 검증

- Maeklog 단위 테스트: 113개 통과.
- 설치된 Maeklog 실행 파일의 비모델 통합 검사 9종 통과: client-routing,
  tab-context, opt-out, recovery, branch-memory, daily-policy, panel-menu,
  decision-contract, reducer-diagnostics.
- 격리 HOME의 설치기 검사 통과: dry run, resource 버전 불일치 거부,
  두 실행 링크 갱신, helper 포함, 백업, 설정·기록 보존, 중복 설치 거부.
- 초기 화면 인식이 없는 Codex의 등록과, 같은 PID라도 프로세스 이름이 셸이면
  등록을 거부하는 회귀 시나리오를 client-routing 통합 검사에 추가했다.
  보완된 Maeklog의 fmt/clippy, 단위 113개, 비모델 통합 9종, release 빌드를 모두
  통과한 뒤 최종 설치했다.
- Codex TUI 검사: 5,567개 실행 중 5,525개 통과, 42개 실패, 8개 skip.
  신규 Herdr bridge 검사 통과. 전체 TUI 검사 통과로 표시하지 않는다.
- 실패한 42개를 정확한 공식 원본 TUI 파일로 다시 검사한 결과 모두 동일하게
  실패했다. 수정본에만 남은 실패는 0개다. 원본 비교 후 패치 파일들을
  byte-for-byte 복원했다. 실패 snapshot을 새 기대값으로 승인하지 않았다.
- 후속 원인 분류: 38개는 개발 버전 `0.0.0`을 기준으로 저장된 화면과 실제
  release 버전 `0.159.2`의 차이였다. 31개는 버전 문자열만 달랐고, 나머지
  7개는 긴 버전 문자열의 정렬·줄바꿈이나 버전 경고 표시가 함께 달랐다.
- 나머지 4개는 테스트 프로세스가 상속한 `NO_COLOR=1` 때문에 ANSI 색상 출력이
  생략된 커서 검사였다. 같은 컴파일된 테스트 실행 파일에서 `NO_COLOR`만
  제거하고 네 검사를 개별 재실행하자 모두 통과했다. 소스나 기대 snapshot은
  변경하지 않았다. 증거: `failure-color-diagnosis.json`.
  이는 네 검사의 원인 확인이며, 전체 TUI 검사를 새 환경에서 다시 통과시켰다는
  의미는 아니다. 최초 실행의 42개 실패 결과와 구분한다.
- Cargo.lock은 배포 tag의 workspace package 159개의 버전만 `0.0.0`에서
  `0.159.2`로 정규화됐다. 외부 의존성이나 의존 그래프 변경은 없었다.
  `just bazel-lock-update`도 실행했으며 `MODULE.bazel.lock` 변경은 없었다.
- `just fix -p codex-tui` 통과. 원본 core의 unused import warning은 수정 범위에
  포함하지 않았다. formatter/linter 실행 후 연동 패치의 byte가 그대로임을 확인했다.
  패치 SHA-256: `16dbdeaeaeb49c4b20fb988ad479b211139791432e22a4b719e9e482add8077f`.
- 최적화 release 빌드 성공: 31분 03초. 기존 방식대로 최종 CLI crate에는
  `-C lto=off`를 적용했다. 원본 app-server/cloud-tasks 경고는 별도로 남겼다.

검증 로그는 `/path/to/private/validation-artifact`에
보관한다. 실행 중인 다른 사용자의 대화 본문을 로그로 복사하지 않았다.

## 실제 연동 검증 방법

`scripts/test-codex-client-live.py`는 실제 Herdr의 임시 sibling pane에서
별도 공식 0.159.2 app-server와 수정 TUI를 연결한다. 서버에는 의도적으로
틀린 workspace/tab/pane 환경을 주고, Maeklog 저장소와 Codex home은 격리한다.
모델 응답은 localhost HTTP SSE fixture가 제공한다. 사용자 인증이나 실제
대화는 복사하지 않으며 외부 모델을 호출하지 않는다.

검증 항목은 신규 대화 등록, 실제 UserPromptSubmit/Stop 수집과 올바른 탭 귀속,
`/new` 등록 갱신, `/quit` 해제, 저장된 대화 resume 및 후속 턴 수집이다.
사용한 테스트 pane과 서버만 종료하고 검증 자료는 별도 디렉터리에 남긴다.

실제 검증은 새 대화와 resume 모두 첫 입력 전 등록을 확인하고, 이후 입력의
요청·답변이 저장되는지 검사한다. 진단용 bridge wrapper를 사용한 후보 검증과
`--default-bridge`를 사용한 설치 경로 검증을 구분한다.

최종 결과:

- 후보 Maeklog adapter: `live-pid-candidate/result.json`, 전체 lifecycle 통과.
- 설치된 `codex`와 기본 Maeklog adapter 경로:
  `live-pid-installed-2/result.json`, 전체 lifecycle 통과.
- 각 실행에서 사용자 요청 2개와 최종 답변 2개가 정확한 탭에 수집됐다.
- 자동 입력은 Herdr가 인식한 idle/done 상태가 안정된 뒤 제출한다. Codex의
  시작 시 typeahead 보호가 이른 Enter를 버리는 동작을 제품 수집 실패와 구분했다.
- 검증용 pane과 별도 app-server는 정상 종료했다.

## 적용 상태

- 최종 적용 package: `$HOME/.local/share/maeklog/codex-0.159.2-maeklog.2`.
- `~/.local/bin/codex`와 `~/.local/bin/codex-maeklog` 모두 새 package의
  `bin/codex`를 가리킨다. 두 명령의 버전 출력은 `codex-cli 0.159.2`다.
- Codex SHA-256: `00d18dc16b34081f7aab7575c88eb991eb323240a3b054f1061e1e63a661bc1c`.
- 공식 code-mode helper SHA-256:
  `5b2c075ac2380fa04d76d7313fbc044d29c8d0a0d0b9138415acd4610211ca03`.
- Maeklog hook SHA-256: `056d83632dbbd286bacf0d04c02a01b556f020fcd23bb3c47cd0e6ce32714d63`.
- 최종 백업: `/path/to/private/installation-backup`.
  `restore-manifest.json`에 기존 두 링크와 hook/config 백업을 기록했다.
  공식 npm 링크를 포함한 최초 전환 전 백업은
  `/path/to/private/installation-backup`에 있다.
- 설치 전후 config.toml, hooks.json, 설치된 Maeklog main 내용의 hash가 같다.
  Maeklog hook만 초기 인식 경합을 보완한 검증 바이너리로 갱신했다.
  기존 routing registry는 초기화하지 않았다. 공식 npm package와 이전 로컬
  0.157.1 package도 유지했다. 사용자 CLI·공유 daemon·Maeklog 패널은 재시작하지 않았다.
- 설치된 모든 실행 파일의 hash를 package의 `maeklog-install.json`과 대조했다.

## 업데이트 시 주의

이 연동은 공식 배포에 포함되지 않은 로컬 패치다. 다음 npm 업데이트는
`codex` 링크를 다시 공식 실행 파일로 바꿀 수 있으며 연동 패치를 자동으로
이식하지 않는다. 다음 버전도 공식 출처 계약 확인 → 패치 이식 → 검사 →
패키지 적용 순서로 갱신해야 한다.

이미 실행 중인 CLI는 링크를 바꿔도 기존 코드로 계속 실행된다. 작업을 끝낸 뒤
정상 종료하고 같은 Herdr 터미널에서 `codex resume`으로 해당 대화를 재개해야
새 기능이 적용된다. 수집되지 않은 과거 대화는 자동으로 복구하지 않는다.
