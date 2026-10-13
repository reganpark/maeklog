# Codex 공유 서버의 자동 탭 연결 검증

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

## 구현

- Codex `rust-v0.157.1` / `36650394c5b38c2990ccf2a3457165ca3e9d9726`의 터미널 클라이언트에 선택적인 Maeklog adapter를 추가했다.
- 대화 활성화와 사용자 턴 제출 전에 클라이언트가 자신의 PID와 대화 ID를 전달한다. 화면 전환과 정상 종료 시 연결을 해제한다. 모델 지시문에는 정보를 넣지 않는다.
- Maeklog adapter는 Herdr에서 실제 foreground Codex PID와 terminal ID를 확인하고, 기존 `pane.report_agent_session`으로 세션을 등록한 뒤 결과를 다시 확인한다. Herdr 자체 소스 변경은 없다.
- 훅은 클라이언트가 등록한 Herdr 실행 파일·socket·session을 사용해 live pane을 조회한다. daemon의 workspace/tab/pane/PATH를 대화 출처로 쓰지 않는다.
- 수집된 턴의 원래 탭을 별도로 유지하므로 대화 전환·pane 이동 뒤의 늦은 답변이 다른 탭으로 넘어가지 않는다. 새 요청은 이동한 pane의 현재 탭에 연결된다.

소스 패치와 재현 방법: [Codex client integration](../integrations/codex/README.md).

## 확인된 범위

- Maeklog 단위 테스트 112개, fmt, clippy, release build, RustSec audit.
- 새 client-routing 통합 검사: 잘못된 daemon 환경과 Herdr 환경이 없는 daemon, 같은 cwd의 다른 대화, native 세션 등록 확인, 오래된 native 보고 거부 및 재등록, 전환 후 늦은 답변, 오래된 detach, 중복 viewer, pane 이동, opt-out, 프로세스 교체, 종료 후 수집 거부.
- 기존 tab-context, opt-out, recovery, branch-memory, daily-policy, panel-menu, decision-contract, reducer-diagnostics 비모델 회귀 검사.
- 실제 Herdr의 임시 검증 pane에서 공유 서버에 연결한 수정 CLI의 신규 대화 등록, `/new` 연결 갱신, 정상 종료 해제를 확인했다.
- 외부 모델 대신 localhost 모의 Responses endpoint로 검증용 대화를 만들고 `resume`을 확인했다. 실행 중 pane을 작업공간 사이에서 이동한 후 다음 입력 전에 연결이 갱신되는 것도 확인했다.
- 실제 Herdr를 대상으로 daemon의 Herdr 환경을 제거하고 잘못된 실행 파일 경로를 준 합성 훅의 요청·완료 수집도 확인했다. 데이터는 격리한 임시 Maeklog 저장소만 사용했다.
- 검증용 Codex 대화는 보관 처리하고, 만든 pane은 종료했다. 기존 사용자 대화나 workspace는 종료하지 않았다.

## 전체 Codex 검사와 한계

수정본의 `just test -p codex-tui` 결과는 5,471개 실행 중 5,422개 통과, 48개 실패, 1개 시간 초과, 8개 skip이었다. 신규 bridge 전달 테스트는 통과했다. 이 결과를 전체 TUI 검사 통과로 표시하지 않는다.

실패에는 릴리스 버전 `0.157.1`과 기준 snapshot의 `0.0.0` 차이, 기존 cursor/paste 검사 등이 포함된다. 기존 snapshot을 새 결과로 덮어 승인하지 않았다.

수정한 TUI 파일들을 백업한 뒤 정확한 원본 tag의 파일로 되돌려, 앞선 실행에서 한 번이라도 실패하거나 시간 초과가 발생한 51건을 비교했다. 원본은 5개 통과·46개 실패였으며, 46개는 수정본의 최종 실패와 동일했다. 수정본에서만 남았던 analytics 시간 초과 1건과 paste 검사 2건은 수정본에서 부하를 줄여 개별 재실행해 모두 통과했다. 신규 bridge 테스트도 이 실행에서 다시 통과했다. 비교 후 수정 소스의 모든 byte와 앞서 생성된 최적화 TUI library hash가 그대로 복원·유지됐음을 확인했다.

첫 Codex 릴리스 빌드의 최종 whole-program LTO 작업은 종료 코드 143으로 중단됐다. 원인을 컴파일 오류로 단정하지 않는다. 재빌드는 일반 release 최적화를 유지하고 최종 CLI crate에만 `-C lto=off`를 적용해 성공했다(13분 03초). 이 로컬 빌드 옵션은 공식 배포 바이너리와의 차이이며, 기능 테스트 결과와 구분한다.

Maeklog의 scheduler lock 테스트는 동시 검사 중 한 차례 실패했고, 단독 재실행에서 통과했다. 최종 Maeklog 단위 검사는 `--test-threads=1`로 수행했다.

같은 Codex 대화를 여러 terminal에서 동시에 열면 새 턴의 제출자를 훅만으로 구분할 수 없다. 이 경우 수집을 거부하고 진단을 남긴다. 이미 출처가 정해진 턴은 원래 출처를 유지한다. 이 제한을 완전한 다중 viewer 지원으로 표현하지 않는다.

## 적용 원칙

설치기는 기존 파일을 먼저 백업하고, 별도의 완전한 Codex package를 준비한다. 변경하는 사용자 설정은 기존 Herdr SessionStart 훅의 `enabled=false` 한 항목이다. Maeklog 훅의 명령·신뢰 hash, 모델 설정, 기존 원장·수집 설정은 변경하지 않는다.

실행 중인 표시 패널의 main 파일은 유지하고, 수집 및 client adapter 역할의 설치된 hook 파일을 갱신한다. 패널/worker 저장 계약은 바뀌지 않았다. main 파일을 교체해 기존 패널의 `current_exe()` worker 실행 경로가 사라지는 일을 피한다. 새 Maeklog 빌드도 별도 package에 보관한다.

이미 실행 중인 CLI는 정상 종료·재개해야 새 client adapter를 사용한다. 과거에 잘못 귀속된 기록은 자동 재분류하거나 재요약하지 않는다. Codex 일반 shell 도구의 daemon 환경 문제까지 고친 것은 아니다.

## 로컬 설치 결과

`scripts/install-client-routing.py`의 dry run 후 `--apply`로 설치했다.

- Codex package: `$HOME/.local/share/maeklog/codex-0.157.1-maeklog.1`
- 실행 링크: `$HOME/.local/bin/codex` → 위 package의 `bin/codex`
- Codex SHA-256: `333dcf5c3e5b41dd2abce78b4955e537cb18e863adf44b23468371fbca542ff7`
- 설치된 Maeklog hook SHA-256: `49be06996ac4df69351e6a83dfbf6f9a5626aec19095399365fe3cfedabe3f37`
- 백업: `/path/to/private/installation-backup`
- 백업의 `restore-manifest.json`에 교체 전 hook/config 파일과 원래 Codex symlink 대상을 기록했다. package의 `maeklog-install.json`에 설치 hash와 기준 commit을 기록했다.

설치 후 실행 링크·바이너리 hash·`codex-cli 0.157.1`을 확인했다. 설정 TOML을 백업과 비교하여 기존 native Herdr SessionStart의 `enabled=false` 외에는 차이가 없음을 확인했다. `hooks.json`과 실행 중 패널이 쓰는 `target/release/maeklog`의 hash도 설치 전후 동일했다. 공식 package와 공유 daemon은 교체·재시작하지 않았다.

설치된 release 실행 파일을 별도의 임시 pane에서 실행해 native Herdr 등록과 실제 Maeklog registry의 PID·terminal ID·대화 ID 일치를 확인했다. `/quit` 후 shell 복귀와 registry 연결 해제를 확인하고 검증 pane을 닫았다. 첫 smoke pane은 종료 입력 직후 너무 일찍 확인·닫아 정상 해제 확인에 사용하지 않았으며, 그 테스트에 한정된 잔여 client metadata만 잠금 아래 제거했다. 모델 요청이나 Maeklog 대화 본문은 생성하지 않았다.

이것은 공식 Codex 배포가 아닌 로컬 수정본이다. 향후 Codex를 업데이트하면 새 버전에 패치를 재적용하고 검증해야 한다. 이전 클라이언트는 연결 정보를 보내지 못하므로 정상 종료 후 `codex resume`으로 재개하기 전까지 새 Maeklog 수집이 거부될 수 있다.
