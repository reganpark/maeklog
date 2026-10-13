# 긴 작업 진행 표시·요청형 요약 적용 — 2026-10-02

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

사용자의 적용 승인 후 **Maeklog main/hook, 열린 표시 패널 4개와 Codex 0.160.0 진행 브릿지**를 로컬 설치했다. 자동 진행 표시는 추가 모델 호출 없이 공개 기록을 보여 주며, `s` 또는 메뉴의 ‘여기까지 요약’을 눌러야 잠정 요약을 생성한다. 시간 간격으로 요약하는 타이머는 없다.

기존 Codex 세션은 정상 종료 후 같은 터미널에서 재개해야 새 브릿지를 사용한다. 설치 전 중간 기록을 소급 복원하지 않는다. [사용 흐름·기존 요약과의 관계·한계](live-progress.md).

## 검증한 Maeklog과 적용 범위

10월 1일 후보와 정확히 같은 source를 persistent target에서 다시 빌드했다. `./scripts/check`의 fmt, clippy `-D warnings`, 단위 128개, release와 cargo audit가 통과했다. 복구한 후보의 live-progress 및 client-routing 통합 검사도 통과했다. 앞선 [12개 비모델 통합·화면 검증](verification-live-progress-2026-10-01.md)의 실행파일과 SHA-256이 같다.

| 항목 | 값 |
|---|---|
| Maeklog version | `0.14.0` |
| main/hook 및 4개 새 패널 실행파일 SHA-256 | `e8b49887c64d1202312bb1366d1062ec8d890b293954de5fcc10b20ef29b79d1` |
| persistent 후보 | `~/.cache/maeklog/live-progress-20261002/target/release/maeklog` |
| 설치 main | `target/release/maeklog` |
| 설치 hook | `~/.config/herdr/plugins/config/maeklog/bin/maeklog-hook` |
| 진행 요약 schema SHA-256 | `94a41aff8990024ecedcad0bfce9abefeeb0b3b8e6de1bb88dc1263cd4b274a3` |
| Maeklog 실행파일 백업 | `/path/to/private/installation-backup` |

[Codex 0.160.0 설치·native producer/receiver 검증](verification-codex-0.160.0-2026-10-02.md) 후 표시 패널만 갱신했다. 표시 프로세스의 label·agent 부재·실제 foreground PID/argv/hash, 원래 tab 및 수집 설정을 확인했다. 현재 사용 중인 pane이나 저장 전 정정 화면은 갱신 대상에서 제외하는 사전 검사를 적용했다.

| 탭 | 이전 Maeklog pane | 새 Maeklog pane | 새 PID |
|---|---|---|---|
| demo-shop `<runtime-id-18>` | `<runtime-id-25>` | `<runtime-id-26>` | <process-id> |
| demo-space `<runtime-id-19>` | `<runtime-id-27>` | `<runtime-id-28>` | <process-id> |
| demo-space `<runtime-id-20>` | `<runtime-id-29>` | `<runtime-id-30>` | <process-id> |
| demo-business `<runtime-id-2>` | `<runtime-id-31>` | `<runtime-id-32>` | <process-id> |

main을 atomic replace하고 같은 탭의 sibling을 대상으로 `--no-focus` 패널을 열었다. 4개 `/proc/PID/exe` hash가 후보와 같고 각 탭의 UI가 자기 새 pane을 가리킴을 확인했다. `<runtime-id-30>`의 `s 여기까지 요약` footer와 F10 메뉴의 같은 항목을 확인한 뒤 Esc로 메뉴만 닫았다. 요약 요청 키나 메뉴 실행은 누르지 않았다.

## 유지한 상태

수집 enabled/generation/history_blocked, 기존 날짜 수동 요청 파일, Codex config와 hooks가 적용 전후 같다. 포커스 `<runtime-id-33>`도 유지됐다. 기록 삭제, 기존 원장 덮어쓰기, 꺼진 탭 재활성화, `r`/`s`/`b` 요청, 재시도 예산 초기화와 훅 재등록은 하지 않았다.

기존 Codex 및 공유 server를 종료하지 않았다. main을 교체하기 전에 old-main/deleted/mismatched worker가 있는지 확인한다. 정확한 E8 설치 hook으로 동작하는 worker는 교체하지 않는 그 실행파일을 계속 사용할 수 있다. 이번 실제 적용 시점에는 active worker가 없었으며 기존 처리 프로세스를 강제로 종료하지 않았다. 새 패널은 원래의 정상 pending 복구 정책을 사용한다.

검증과 메뉴 확인에는 외부 모델 호출이 없다. 실사용 Luna의 잠정 요약 품질은 별도 평가하지 않았다. 요청형 요약은 GPT-6 Luna / medium, Fast 꺼짐, 180초 및 기존 동시 처리 한도를 사용한다. 잠정 결과가 최종 요약의 입력 재독까지 없애는 것은 아니다.

private 근거: `/path/to/private/validation-artifact`. `deployment-result.json`, 원래/새 pane metadata와 실행 hash, protected setting 비교, `menu-smoke.json`, 기존 process 확인과 GPT-6.1 Sol / max 독립 검토를 보관한다. 원본 source·동작·화면 검증은 10월 1일 기록을 따른다.
