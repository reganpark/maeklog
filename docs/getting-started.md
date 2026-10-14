# 설치와 최초 확인

[종합 한국어 설치·설정 가이드](setup.ko.md)는 영어 setup과 같은 소스 복제·빌드·연결(`clone`/`link`) 경로를 안내합니다. 이 문서는 Herdr의 `plugin install` 경로를 기준으로 설명합니다.

Maeklog는 Herdr 안에서 Codex 대화를 탭별로 정리하는 플러그인입니다. 처음에는 **플러그인 설치 → 네 훅 등록·신뢰 → 준비 상태 확인 → 통합 Codex 설치 → 새 대화 확인** 순서로 진행합니다.

현재는 소스 빌드 방식입니다. Herdr 플러그인과 Codex 연결을 모두 설치해야 합니다.

## 준비물과 사용량

- Linux x86_64, Herdr 0.8.2 이상. macOS는 실험적 지원 대상으로, 실기 검증은 아직 진행하지 않았습니다.
- 로그인한 공식 Codex CLI. 통합 패치의 기준 버전은 0.162.1입니다.
- Python 3.11 이상, Git, GitHub CLI(`gh`), Rust/rustup, `just`, `cargo-nextest`, Node.js/npm, Bazel 9.2.0 또는 Bazelisk. `bazel` 명령이 PATH에서 실행돼야 합니다. Codex 0.162.1은 Bazel 9.2.0을 요구하며, Bazelisk는 소스에 지정된 버전을 선택할 수 있습니다.
- Maeklog의 [Rust toolchain](../rust-toolchain.toml)은 1.97.1로 고정돼 있습니다. 통합 Codex는 내려받은 upstream 소스의 toolchain을 사용합니다. 소스 빌드와 의존성 다운로드에 필요한 시간과 디스크 공간을 확보하세요.

요약에는 **추가 모델 사용량**이 발생하며 금액 기준 예산 상한은 없습니다. 수집한 기록은 로컬에 저장하고, 요약할 때 대화와 일부 기존 기록을 모델 서비스에 전송합니다. 새 탭의 수집은 기본으로 켜져 있으며 자동 요약은 오늘 수집된 완료 대화로 제한됩니다. 수집을 중지하려면 패널 메뉴의 **이 탭 수집 끄기**를 사용하세요. [저장·삭제와 사용 범위](usage.md#모델-사용량과-저장)를 먼저 확인해도 좋습니다.

## 1. Herdr 안에서 플러그인 설치하기

Herdr를 실행하고 그 안의 터미널에서 입력합니다.

```sh
herdr plugin install reganpark/maeklog --ref v0.14.0
herdr plugin list --plugin maeklog --json
```

Herdr의 설치 화면에서 소스와 빌드 명령을 확인합니다. 목록에 ID가 `maeklog`인 플러그인이 표시돼야 합니다. 다운로드한 소스가 이미 있다면 그 소스 디렉터리에서 직접 빌드한 뒤 연결할 수도 있습니다. `plugin link`는 빌드 명령을 실행하지 않습니다.

```sh
# 받은 Maeklog 소스 디렉터리에서 실행
cargo build --locked --release
herdr plugin link "$PWD"
```

다음 명령은 목록에서 Maeklog의 실제 설치 경로를 찾아 `maeklog_root`에 담습니다. 후보가 정확히 하나일 때만 경로를 선택합니다. 이후 명령은 같은 터미널에서 계속 실행하세요.

```sh
maeklog_root="$(herdr plugin list --plugin maeklog --json | python3 -c '
import json, sys
matches = [p for p in json.load(sys.stdin)["result"]["plugins"]
           if p.get("plugin_id", p.get("id")) == "maeklog"]
len(matches) == 1 or sys.exit("Expected exactly one Maeklog plugin")
print(matches[0]["plugin_root"])
')"
"$maeklog_root/target/release/maeklog" --version
```

`maeklog 0.14.0`이 표시되면 Maeklog 바이너리 빌드가 완료된 상태입니다. 파일을 찾을 수 없다면 먼저 `herdr plugin log list --plugin maeklog`로 빌드 오류를 확인하세요.

## 2. Codex 훅 등록하고 직접 신뢰하기

```sh
herdr plugin action invoke install-codex-hooks --plugin maeklog
```

이 동작은 Maeklog 훅 실행 파일과 실행 설정을 만들고, Codex의 `hooks.json`에 네 이벤트의 핸들러를 등록합니다. 기존 훅 파일은 변경 전에 백업하며, 다른 핸들러는 보존합니다.

Codex의 hooks 기능이 꺼져 있다면 다음 명령으로 활성화하고 새 Codex 세션을 엽니다.

```sh
codex features enable hooks
codex
```

Codex 안에서 `/hooks`를 열어 아래 **Maeklog 핸들러 네 개를 각각 확인하고 신뢰**합니다. 명령은 Maeklog 설정 디렉터리의 `bin/maeklog-hook`을 실행하며 마지막 인자는 `hook`입니다.

| 이벤트 | 수집 대상 |
| --- | --- |
| `SessionStart` | 세션 연결 |
| `UserPromptSubmit` | 사용자 요청 |
| `Stop` | 최종 답변 |
| `Interrupt` | 중단 사실 |

설치 도구는 이 신뢰를 대신 작성하지 않습니다. 소스나 명령이 달라져 신뢰가 유효하지 않으면 `/hooks`에서 새 정의를 다시 확인하세요. 이미 실행 중인 작업은 끝낸 뒤 정상 종료합니다.

## 3. 준비 상태 확인하기

Codex 입력창을 정상 종료하거나 별도 **Herdr 일반 셸**로 돌아와 실행합니다.

```sh
python3 "$maeklog_root/scripts/update-codex-maeklog.py" --preflight
```

이 확인은 다운로드·빌드·외부 모델 호출을 하지 않습니다. 필수 명령, Herdr 호출 위치, 설치된 Maeklog 실행 설정·훅, hooks 활성화, 네 핸들러의 현재 정의와 수동 신뢰 상태를 확인합니다.

오류가 있으면 표시된 항목부터 해결하고 다시 실행합니다. `--check`는 버전 정보만 보는 옵션이며 이 준비 상태 확인을 대신하지 않습니다.

`XDG_CONFIG_HOME`이나 `CODEX_HOME`을 바꿔 쓰는 경우 훅 등록·신뢰·업데이트·Codex 실행에 같은 환경을 유지해야 합니다. 별도 `HERDR_PLUGIN_CONFIG_DIR`가 기본 브릿지 경로인 `$XDG_CONFIG_HOME/herdr/plugins/config/maeklog` 밖을 가리키는 구성은 이 설치 흐름에서 지원하지 않으며 준비 확인에서 중단합니다. XDG 설정이 없으면 기본 경로는 `~/.config/herdr/plugins/config/maeklog`입니다.

## 4. Maeklog용 Codex 빌드와 연결하기

```sh
python3 "$maeklog_root/scripts/update-codex-maeklog.py" 0.162.1 --apply
```

이 명령은 공식 Codex 소스와 일치하는 helper/resource 패키지를 내려받아 Maeklog 패치를 적용하고 빌드합니다. 로컬 응답 fixture로 연결·수명 주기를 확인한 뒤 설치를 활성화합니다. 검증용 임시 Herdr 창을 만들고 닫으며 외부 모델을 호출하지 않습니다. 준비나 검증이 실패하면 활성 설치를 교체하지 않습니다.

설치 계획까지만 보려면 같은 명령에서 `--apply`를 생략합니다. 생략해도 다운로드·빌드·검증은 진행합니다.

```sh
python3 "$maeklog_root/scripts/update-codex-maeklog.py" 0.162.1
```

활성화할 때 기존 실행 파일·런처와 설정을 백업합니다. 이전 Herdr 서버 측 `SessionStart` 핸들러가 있으면 확인된 해당 핸들러의 enabled 상태만 끄고, Maeklog의 터미널 클라이언트 보고를 사용합니다. 이미 실행 중인 Codex와 공유 서버는 재시작하지 않습니다.

설치 후 **새 Herdr 일반 셸**에서 `~/.local/bin`을 PATH 앞에 두고 확인합니다.

```sh
export PATH="$HOME/.local/bin:$PATH"
```

이 설정은 현재 셸에 적용됩니다. 계속 사용하려면 자신이 쓰는 셸의 시작 파일에 같은 줄을 추가합니다. 예를 들어 Bash는 `~/.bashrc`, Zsh는 `~/.zshrc`를 쓰며, 로그인 셸 등 별도 구성이 있으면 그 설정을 따릅니다. 새 Herdr 터미널에서 아래 경로를 다시 확인하세요.

```sh
command -v codex
command -v codex-maeklog
codex --version
codex-maeklog --version
```

`codex`와 `codex-maeklog`의 경로는 각각 `~/.local/bin/codex`, `~/.local/bin/codex-maeklog`이며 두 버전 출력은 `codex-cli 0.162.1`이어야 합니다. 셸이 이전 실행 파일 경로를 기억하면 새 터미널을 열거나 해당 셸의 명령 캐시를 새로고침하세요. 버전만 같다고 패치된 빌드라고 판단하지 말고 준비 확인과 런처 경로도 함께 확인합니다.

`codex-maeklog`는 패키지 경로·버전 확인용 포인터입니다. 대화형 작업은 설치된 `codex` 런처로 실행하세요. PATH가 다른 명령을 선택하면 `~/.local/bin/codex`를 직접 실행할 수 있습니다. 그 파일 자체가 덮였다면 같은 설치된 런처 스크립트를 다음처럼 실행합니다.

```sh
python3 "$HOME/.local/share/maeklog/codex-launcher.py"
# 같은 대화 재개
python3 "$HOME/.local/share/maeklog/codex-launcher.py" resume
```

플러그인 checkout은 설치 뒤에도 유지하세요. `maeklog-update`는 이 소스의 업데이트 스크립트를 사용합니다.

## 5. 첫 대화로 확인하기

1. Herdr의 작업 탭에서 `codex`를 실행하거나 `codex resume`으로 대화를 재개합니다. 설치 전에 열린 Codex는 작업 후 정상 종료하고 다시 실행하세요.
2. 같은 탭에서 패널을 엽니다.

   ```sh
   herdr plugin action invoke open --plugin maeklog
   ```

3. 민감한 정보가 없는 짧은 요청을 주고받습니다. 예를 들어 “이 작업의 진행 상황과 다음 행동을 알려 주세요”처럼 요청할 수 있습니다. 이 대화가 완료되면 별도 요약 사용량이 발생할 수 있습니다.
4. 패널 위쪽에서 공개 진행을 확인하고, 응답이 끝난 뒤 아래 저장 기록의 반영 상태를 확인합니다. 공개 설명이 없는 응답은 **진행 정보 없음**으로 보일 수 있습니다. 보고가 끝났다는 사실과 작업 완료는 구분합니다.
5. 패널을 클릭해 `h`로 전체 기록을 읽고, 항목이 있으면 `e`로 근거를 확인합니다. 아직 반영되지 않은 오늘 완료 대화는 `r`로 요청합니다.

처음 연결하기 전의 대화나 수집을 꺼둔 동안의 대화는 자동으로 가져오지 않습니다. 패널에 결정이나 완료 항목이 항상 생기는 것도 아닙니다. 실제 확정·완료 보고가 있는 대화에서만 기록을 남깁니다.

## 6. 열기·숨기기 단축키 설정

처음에는 위의 `open` 명령으로 확인하고, 자주 쓴다면 Herdr 설정 파일의 키 목록에 다음 항목을 추가합니다. 기본 파일은 `~/.config/herdr/config.toml`이며 `HERDR_CONFIG_PATH`로 바꿀 수 있습니다. 실제 경로는 `herdr --help`의 `Config` 항목에서 확인합니다. 기존에 같은 키를 사용하고 있으면 중복 항목을 추가하지 말고 원하는 동작으로 수정하세요.

```toml
[[keys.command]]
key = "prefix+m"
type = "plugin_action"
command = "maeklog.open"
description = "open Maeklog"

[[keys.command]]
key = "prefix+shift+m"
type = "plugin_action"
command = "maeklog.hide"
description = "hide Maeklog"
```

저장한 설정을 검사하고 적용합니다.

```sh
herdr config check
herdr server reload-config
```

기본 prefix는 `Ctrl+B`입니다. `prefix+m`은 `Ctrl+B`를 누른 뒤 `m`을 차례로 누르고, `prefix+Shift+m`은 prefix 뒤 대문자 `M`을 누릅니다. prefix를 바꿨다면 자신의 설정을 사용하세요.

`q`·패널 숨기기는 수집을 계속합니다. 수집과 요약을 중지하려면 `m` 메뉴의 **이 탭 수집 끄기**를 고릅니다. 다시 열거나 켜면 수집해 둔 오늘의 완료 대화 중 최근 최대 3턴부터 처리하고 나머지는 `b`의 수동 보류로 남깁니다.

## 업데이트

Herdr 터미널에서 버전을 확인하고 업데이트합니다.

```sh
codex update --check
codex update 0.162.1
```

`codex update`는 Maeklog의 검증된 업데이트 흐름을 실행합니다. 새 upstream 버전에 패치가 맞지 않거나 검증이 실패하면 설치를 멈추므로 [Codex 통합 안내](../integrations/codex/README.md)를 확인하세요. 일반 npm 업데이트는 이 통합을 업데이트하지 않으며 `codex` 런처 경로를 바꿀 수 있습니다.

## 문제 해결

| 상황 | 해결 순서 |
| --- | --- |
| `python3`에서 `tomllib`을 찾을 수 없음 | Python 3.11 이상인지 확인합니다. |
| 플러그인 목록이 없거나 후보가 여러 개임 | `herdr plugin list --plugin maeklog --json`에서 ID와 경로를 확인하고 설치·연결 상태를 정리합니다. 경로를 추측하지 않습니다. |
| Maeklog 바이너리가 없음 | Herdr 플러그인 빌드 로그를 읽고 Rust toolchain·의존성 오류를 해결합니다. |
| 훅이 없음·신뢰 불일치 | `install-codex-hooks`를 실행하고 같은 Codex 환경의 `/hooks`에서 네 정의를 검토·신뢰한 뒤 `--preflight`를 다시 실행합니다. |
| 필수 명령·Bazel 누락 | 준비 확인이 표시한 도구를 설치합니다. Bazelisk를 쓰면 `bazel` 명령도 PATH에 있어야 합니다. |
| 통합 빌드·검증 실패 | 출력에 표시된 로그를 확인합니다. 로그·소스·검증 자료는 기본 `~/.local/share/maeklog/validation`에 남습니다. |
| 패널에 새 대화가 안 보임 | 새 Codex가 설치된 런처를 쓰는지, 네 훅의 신뢰·enabled 상태, hooks 기능, `--preflight` 결과를 확인합니다. |
| 같은 대화를 여러 터미널에서 열었음 | 새 요청의 주체가 모호하면 수집을 보류합니다. 한 터미널에서 진행하고 정상 종료·재개로 등록을 갱신합니다. |
| 요약 실패·자동 재시도 중지 | `h`의 **요약·처리 상세**에서 오류를 확인하고 모델 접근 권한·네트워크·설정을 해결합니다. 오늘은 `r`, 선택한 이전 날짜는 `b`로 재요청합니다. 원문은 실패 시 보존됩니다. |
| 저장 내용이 잘못됐음 | `e`로 근거를 확인하고 내용·조건을 정정하거나 오기록을 제외합니다. `u`로 사용자 정정을 되돌릴 수 있습니다. |
| 키가 작동하지 않음 | 패널 포커스를 확인합니다. 열기·숨기기 키는 위 설정을 등록했는지 확인하며, 패널 안의 `m` 메뉴로도 기능을 찾을 수 있습니다. |

## 기록 위치 확인

실제 기록 위치는 설치 시점의 `hook-runtime.json`에 저장합니다. Herdr의 기본 플러그인 상태 경로는 `~/.local/state/herdr/plugins/maeklog`이며 일반 CLI의 환경 없는 fallback인 `~/.local/state/maeklog`와 다릅니다. 다음 명령으로 현재 위치를 읽습니다.

```sh
maeklog_config_dir="$(herdr plugin config-dir maeklog)"
python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["state_dir"])' \
  "$maeklog_config_dir/hook-runtime.json"
```

새 설치는 Maeklog 네임스페이스에서 시작합니다. 다른 플러그인의 설정·기록을 자동으로 찾거나 이관하지 않습니다. 기존 기록의 이관을 위해 상태 파일을 임의로 합치지 마세요.

빈 HOME의 설치 smoke 검증, 새 upstream 소스 전체 빌드, 공개 GitHub에서의 플러그인 설치는 서로 다른 확인입니다. 현재 완료한 범위와 남은 확인은 [배포 준비 상태](release-readiness.md)에 기록합니다. Linux의 격리 fixture 검증과 macOS 실기 검증도 구분합니다. 날짜가 붙은 이전 검증 문서는 당시 결과이며 새 환경의 설치 성공을 보증하지 않습니다. 실제 모델의 의미 판단과 응답 품질은 근거 확인·사용자 정정을 함께 사용해 검토하세요.

[사용 안내](usage.md) · [Codex 통합](../integrations/codex/README.md) · [보안 정책](../SECURITY.md)
