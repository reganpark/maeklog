# Codex 0.164.0 후보 연동 검증 — 2026-10-26

**Linux 후보 빌드와 실행 검증 완료. 현재 설치는 변경하지 않았다.** PR #5의 명령 문자열 제외 패치로 updater를 `--apply` 없이 실행했다. 후보 바이너리의 실제 버전은 `codex-cli 0.164.0`이고, updater의 검증 기록은 `applied: false`다.

| 대상 | 값 |
| --- | --- |
| 공식 소스 | `rust-v0.164.0` / `6aaac694803004a63c6f393d153c96d22521a6b9` |
| 패치 | `integrations/codex/codex-0.164.0-maeklog.patch` |
| 패치 SHA-256 | `93176f15be1c6df47238353794a03dd22ddc1bf382c62bfc379dedab4e24595d` |
| 후보 CLI SHA-256 | `bbd1ac7a40bb8f1416d4c77c7d38707bc2f5fdf4597cdd507e34056608b321f5` |

| 검증 | 결과 |
| --- | --- |
| 공식 소스 패치 적용 | PASS |
| 의존성 및 포맷 | 외부 의존성 graph 유지 검사, Bazel 9.2.0 metadata 동기화, 포맷 PASS |
| `codex-tui`의 `herdr_bridge` 테스트 | 7개 PASS, 무관한 5,852개 skipped |
| 명령 문자열 제외 | 브리지 테스트의 자격증명 표본이 출력 metadata에서 제외됨. Maeklog 수신부의 저장·요약 대기 데이터 제외 회귀도 PASS |
| TUI Clippy | PASS. 수정하지 않은 `app-server/src/message_processor.rs`의 `let_and_return` 경고 유지 |
| 실제 CLI 릴리스 빌드 | PASS, 30분 59초. 수정하지 않은 app-server의 `unused_mut`와 cloud-tasks의 미사용 import 경고 유지 |
| installer / launcher | 각각 25개 / 11개 PASS |
| client routing | terminal/tab 분리, 오래된 detach, opt-out, 교체된 client 거부 등 PASS |
| 실제 Herdr lifecycle | 시작·등록, prompt/final hook 수집, `/new`, 정상 종료·detach, resume·재등록, 재개 후 turn·detach PASS |
| native progress | attach → commentary·계획·partial answer → 완료 → detach PASS. final answer의 중복 progress 수집 없음 |
| native progress 격리 | Maeklog 요약 모델 호출과 durable queue 기록 없음 |
| 설치 전 검사 | PASS. 후보를 활성 설치로 교체하지 않음 |

재현 경로는 다음과 같다. `--pane`은 검증 시점의 실제 호출 pane ID를 사용한다.

```sh
python3 scripts/update-codex-maeklog.py 0.164.0 --pane CALLING_PANE_ID --jobs 4
python3 scripts/test-codex-live-progress.py \
  --binary CANDIDATE/bin/codex \
  --server-binary OFFICIAL_RESOURCE_PACKAGE/bin/codex \
  --pm-binary /path/to/current/maeklog \
  --root /path/to/new/private/native-progress \
  --partial-answer
```

별도로 `scripts/test-opt-out.mjs`의 기존 incremental/latest 검증과 새 resize 오류·실제 8초 타임아웃 검증을 통과했다. 두 실패 경로 모두 `open`이 성공하고 `ui.json`에 패널 ID와 표시 상태를 저장하며, 다음 `open`은 패널을 중복 생성하거나 resize를 재시도하지 않는다. 타임아웃된 모의 resize 프로세스의 종료도 확인했다.

실행 테스트는 격리된 상태와 localhost 응답 fixture를 사용했으며 외부 모델 추론을 호출하지 않았다. 전체 upstream TUI suite, 새 기기 최초 설치, macOS, 후보의 실제 설치·활성화는 이번 검증 범위가 아니다. 원본 로그·상태·설정은 로컬 private evidence에만 보존하고 공개 파일에는 포함하지 않는다.
