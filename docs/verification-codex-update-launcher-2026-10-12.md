# Codex update 런처 적용 — 2026-10-12

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

`codex update`를 기존 Maeklog updater의 `--apply` 절차로 연결하는 런처를 적용했다. 일반 `codex` 명령은 인자를 그대로 실제 연동 바이너리에 전달하며 PID를 유지한다. 이번 적용은 **런처만 설치**했으며 기존 Codex 0.161.0 바이너리를 다운로드하거나 다시 빌드하지 않았다.

```sh
python3 scripts/install-client-routing.py --launcher-only --apply
```

| 설치 항목 | 적용 결과 |
| --- | --- |
| `codex` | `~/.local/share/maeklog/codex-launcher.py`로 연결 |
| `codex-maeklog` | 기존 `~/.local/share/maeklog/codex-0.161.0-maeklog.1/bin/codex` 유지 |
| `maeklog-update` | 기존 checkout의 `scripts/update-codex-maeklog.py` 연결 유지 |
| 런처 SHA-256 | `7826ab71c80b83c3792ed7b164b24f42f2b07878f866f24811cc1be5712696aa` |
| 설치 백업 | `/path/to/private/installation-backup` |

적용 전후 `config.toml`, `hooks.json`, 실제 Codex 바이너리와 설치된 Maeklog hook의 SHA-256이 모두 같았다. 기존 manifest의 바이너리 해시는 보존하고 런처 파일·링크·해시는 별도로 검증한다. 적용 후 launcher-only 사전 확인은 `already_current: true`, `replace: []`를 반환했다. 공유 app server와 활성 작업은 재시작하지 않았다.

| 검증 | 결과 |
| --- | --- |
| `python3 scripts/test-codex-launcher.py` | 11개 PASS: 인자 전달, native PID·프로세스명·argv0, 반환 코드, update/help/check/version, 경로·누락 오류 |
| `python3 scripts/test-install-client-routing.py` | 16개 PASS: 버전·resource gate, 백업, 이관·다음 설치, config·registry 보존, 실패 복구·동시 registry 기록 보존 |
| `python3 scripts/test-update-codex-maeklog.py` | 13개 PASS: 적용 전 실패 gate, 별도 런처·바이너리 검증, 가짜 HOME에서 설치기 실행·revision 갱신 |
| 기존 Maeklog client routing 회귀 | 가짜 Herdr와 설치된 hook으로 sandbox 밖에서 PASS |

실제 설치본의 `codex --version`은 `codex-cli 0.161.0`이다. `codex update --help`와 `codex update 0.161.0 --check`가 통과했고, `codex update 0.161.0`은 검증된 동일 설치를 확인한 뒤 빌드 없이 종료했다. `codex update --check`의 release 조회는 설치 버전 0.161.0, 대상 0.162.1을 보고했다.

0.162.1의 소스 다운로드·빌드·패치 호환 검사는 수행하지 않았다. 새 릴리스는 패치 적용과 연동 검사에 통과해야 설치되며, 충돌하면 수동 이식이 필요하다. 외부 모델 호출은 없었다. [사용·설치 방법](../integrations/codex/README.md).

로컬 근거: `/path/to/private/validation-artifact`의 `installed-before.json`, `installed-after.json`, `checked-source-hashes.json`.
