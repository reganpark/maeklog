# README 예시 화면

[panel.png](panel.png)는 2026-10-13에 Maeklog 0.14.0의 현재 터미널 UI를 **완전히 가상의 검색 필터 작업 데이터**로 실행한 화면입니다. 실제 사용자 대화나 과거 익명화 캡처를 사용하지 않았습니다. 화면에 나오는 테스트 개수와 작업 보고도 예시 데이터입니다.

- 가상의 결정 1건, 완료 보고 1건, 진행 1건, 다음 행동 1건을 격리 상태 파일에 넣었습니다. 모델이 생성한 기록의 품질 평가가 아닙니다.
- 패널은 실제 바이너리를 PTY의 80×30 셀에서 실행했습니다. [panel.ansi](panel.ansi)는 그 터미널 프레임이며 PNG는 같은 프레임을 렌더링한 것입니다. 화면 문구나 픽셀을 고쳐 넣지 않았습니다.
- HOME·CODEX_HOME·XDG 경로·Maeklog 상태·설정과 Herdr 연결을 자식 프로세스에서 격리했습니다. Git 저장소는 fixture 안의 빈 저장소입니다.
- 모델 실행을 비활성화하고 worker 잠금을 유지했습니다. 모델·GitHub·브라우저 호출은 guard로 막고 호출 수가 0인지 확인했습니다. Herdr 응답도 로컬 mock입니다.

소스 루트에서 재현합니다. 빌드된 Maeklog 바이너리, Node.js, Python 3, Git, Pillow, fontconfig와 Noto Sans Mono CJK KR 글꼴이 필요합니다. 캡처 스크립트는 제품 사용에 필요한 설치 단계가 아닙니다.

```sh
node scripts/capture-readme-demo.mjs \
  target/release/maeklog docs/assets/demo /tmp/maeklog-readme-demo
```

실행 결과와 바이너리·스크립트·PNG 해시는 세 번째 인자의 근거 디렉터리에 `result.json`으로 저장합니다. 캡처 시각과 임시 저장소 식별값은 재실행할 때 달라집니다. [이전 캡처](../../captures/README.md)는 별도의 역사적 참고 자료입니다.
