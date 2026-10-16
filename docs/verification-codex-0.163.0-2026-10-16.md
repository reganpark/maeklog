# Codex 0.163.0 연동 검증 — 2026-10-16

**기존 Linux 설치의 클라이언트 교체 검증 완료.** `codex`와 `codex-maeklog` 모두 `codex-cli 0.163.0`을 출력한다. 설치 후 실제 `codex update`, `codex update 0.163.0`, `codex update --check`는 검증된 동일 설치를 확인하고 추가 빌드 없이 정상 종료했다.

0.162.1 패치는 0.163.0의 변경된 초기화 필드, boxed test fixture와 함수 호출 문맥에 맞지 않았다. 새 패치는 upstream 변경을 보존하며 연동 코드를 이식한다. 네 bridge 모듈 자체와 production bridge의 시간 제한은 이전 패치와 같다.

| 소스 | 값 |
| --- | --- |
| 공식 source | `rust-v0.163.0` / `83086188ae0614bd882818ff1aa23f3194962012` |
| 패치 | `integrations/codex/codex-0.163.0-maeklog.patch` |
| 패치 SHA-256 | `efd624ad89fb9985cebf30dcd4d0b258d6e33486863d05d69126c3ce13abb0a8` |

| 검증 | 결과 |
| --- | --- |
| 패치 | 새 원본 checkout의 apply 검사, 포맷 후 reverse-apply 검사, whitespace 검사 PASS |
| 의존성 | local workspace 버전 정규화 외 공식 release의 외부 의존성 graph 유지. Bazel 9.2.0 metadata 동기화 PASS |
| Rust bridge | 7개 PASS, 무관한 5,791개 skipped |
| TUI Clippy·release build | PASS; 수정하지 않은 upstream 경고는 남아 있음. 후보의 실제 버전 확인 PASS |
| Python 회귀 | hook trust 4개, installer 25개, launcher 11개, updater 17개: 총 57개 PASS |
| 공식 hook metadata | 별도 0.163.0 app server의 `hooks/list`에서 네 승인 fingerprint·enabled·trusted 상태 일치. thread나 모델 turn을 만들지 않음 |
| 공식 resources | CLI를 제외한 helper/resource/package 파일 42개가 공식 package와 byte-identical |
| routing | 설치된 helper를 사용한 terminal/tab 분리 회귀 PASS |
| 실제 Herdr lifecycle | 후보와 설치된 stable launcher 모두 시작, prompt/final hook, `/new`, 정상 종료, resume, 재개 후 turn, detach PASS |
| native progress | commentary·계획·partial answer·완료·detach PASS |
| native task reports | code-mode와 sandboxed helper를 통한 context·Set·Related·receipt·다른 실행 thread ID 거부 PASS. 실제 child turn을 생성한 검사는 아님 |
| 설치 후 | `installation_current=true`; 실제 versioned/unversioned update는 no-op, `--check`는 검증된 설치가 활성 상태임을 출력 |

Task report fixture는 영어·한국어 오류 문구를 허용하고 비동기 exec session의 완료를 기다리도록 고쳤다. 호출마다 Node를 시작하던 모의 Herdr가 bridge의 2초 제한을 넘는 문제는 같은 응답 계약의 Python fixture로 해결했다. 완료된 fixture의 결과와 script hash가 현재 테스트와 일치하며, 앞선 실패 근거도 private 디렉터리에 보존했다.

설치 전후 Codex 설정·훅·설치된 Maeklog helper는 같았고, 기존 Codex 프로세스와 공유 서버의 재시작 없이 새 런처와 패키지를 활성화했다. 기존 세션은 작업 후 정상 종료하고 `codex resume`으로 재개한다.

검사는 로컬 응답 fixture를 사용했고 외부 모델 추론은 호출하지 않았다. 전체 upstream TUI suite, 새 기기의 최초 설치, macOS 또는 이후 upstream 릴리스의 호환성을 검증한 결과는 아니다. 원본 로그·상태·설정·설치 백업은 공개 파일에 포함하지 않고 로컬에만 보존한다. 설치에 필요한 현재 소스와 업데이트 방법은 [통합·업데이트 가이드](../integrations/codex/README.md)를 따른다.
