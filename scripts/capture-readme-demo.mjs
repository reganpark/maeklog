#!/usr/bin/env node
// Capture the current TUI with invented data, an isolated home, and no model calls.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {spawn, spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const project = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const binary = resolve(process.argv[2] ?? join(project, 'target/release/maeklog'));
const output = resolve(process.argv[3] ?? join(project, 'docs/assets/demo'));
const evidence = resolve(process.argv[4] ?? '/tmp/maeklog-readme-demo');
for (const path of [output, evidence]) mkdirSync(path, {recursive: true});
const root = mkdtempSync(join(evidence, 'fixture-'));
const fixtureHome = join(root, 'home'), state = join(root, 'state'), config = join(root, 'config');
const repo = join(root, 'demo-project'), calls = join(root, 'external-calls.jsonl');
for (const path of [fixtureHome, state, config, repo]) mkdirSync(path);
const env = {
  PATH: process.env.PATH, HOME: fixtureHome, CODEX_HOME: join(fixtureHome, '.codex'),
  XDG_CONFIG_HOME: join(fixtureHome, '.config'), XDG_STATE_HOME: join(fixtureHome, '.local/state'),
  XDG_CACHE_HOME: join(fixtureHome, '.cache'), XDG_DATA_HOME: join(fixtureHome, '.local/share'),
  TMPDIR: root, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null',
  MAEKLOG_STATE_DIR: state, MAEKLOG_PLUGIN_ROOT: project, HERDR_PLUGIN_CONFIG_DIR: config,
  HERDR_ENV: '1', HERDR_WORKSPACE_ID: 'demo', HERDR_TAB_ID: 'demo:t1', HERDR_PANE_ID: 'demo:p1',
  HERDR_SESSION: 'readme-demo', HERDR_SOCKET_PATH: '/maeklog-readme-demo/fake.sock',
  HERDR_BIN_PATH: join(project, 'scripts/mock-live-herdr.mjs'),
  PM_CLIENT_TOPOLOGY: join(root, 'topology.json'), MAEKLOG_DISABLE_LUNA: '1',
  MAEKLOG_CODEX_BIN: join(project, 'scripts/mock-language-picker-guard.mjs'),
  MAEKLOG_GH_BIN: join(project, 'scripts/mock-language-picker-guard.mjs'),
  MAEKLOG_OPEN_BIN: join(project, 'scripts/mock-language-picker-guard.mjs'),
  MAEKLOG_LANGUAGE_TEST_CALLS: calls,
  LC_ALL: 'ko_KR.UTF-8', TERM: 'xterm-256color', COLORTERM: 'truecolor', TZ: 'Asia/Seoul',
};
const put = (path, value) => writeFileSync(path, JSON.stringify(value));
const json = path => JSON.parse(readFileSync(path, 'utf8'));
const pause = ms => new Promise(done => setTimeout(done, ms));
const digest = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const plain = value => value.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, '').replace(/\x1b\][^\x07]*\x07/g, '');
const run = async (args, input) => {
  const child = spawn(binary, args, {env, stdio: ['pipe', 'pipe', 'pipe']});
  let stdout = '', stderr = '';
  child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
  child.stdout.on('data', data => stdout += data); child.stderr.on('data', data => stderr += data);
  child.stdin.on('error', () => {});
  child.stdin.end(input === undefined ? undefined : JSON.stringify(input));
  const timer = setTimeout(() => child.kill(), 10000);
  const code = await new Promise((done, reject) => {child.once('close', done); child.once('error', reject);});
  clearTimeout(timer);
  assert.equal(code, 0, stderr || 'Fixture CLI did not finish');
  return stdout.trim() ? JSON.parse(stdout) : {};
};
let guard, panel, spool;
try {
  for (const args of [
    ['init', '-q', '-b', 'search-filters'],
    ['-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '--allow-empty', '-m', 'Synthetic README fixture'],
  ]) {
    const result = spawnSync('git', args, {cwd: repo, env, encoding: 'utf8'});
    assert.equal(result.status, 0, result.stderr);
  }
  put(env.PM_CLIENT_TOPOLOGY, {socket: env.HERDR_SOCKET_PATH, panes: [{pane_id: 'demo:p1', workspace_id: 'demo', tab_id: 'demo:t1', terminal_id: 'demo-terminal', pid: 101, agent: 'codex', cwd: repo}]});
  guard = spawn('python3', ['-c', 'import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()', join(state, 'worker.lock')], {env, stdio: ['pipe', 'pipe', 'inherit']});
  await new Promise((done, reject) => {guard.stdout.once('data', done); guard.once('error', reject);});
  const request = '검색 조건을 바꿀 때 선택한 필터를 유지해 주세요.';
  // Initialize a local synthetic event without querying any terminal; routing is tested elsewhere.
  delete env.HERDR_PANE_ID;
  const hook = await run(['hook'], {hook_event_name: 'UserPromptSubmit', session_id: 'thread-demo', turn_id: 'turn-demo', cwd: repo, prompt: request});
  env.HERDR_PANE_ID = 'demo:p1';
  put(join(state, 'client-routing.json'), {clients: [], turns: {}});
  await run(['client', 'attach', '101', 'thread-demo']);
  if (hook.hookSpecificOutput?.additionalContext) spool = JSON.parse(hook.hookSpecificOutput.additionalContext.split('\n')[0].slice('MAEKLOG_TASK_REPORT_CONTEXT_V1 '.length)).spool;
  const space = readdirSync(join(state, 'spaces')).map(key => join(state, 'spaces', key)).find(path => json(join(path, 'meta.json')).tab_id === 'demo:t1');
  assert(space, 'demo tab was not initialized');
  const seed = Object.values(json(join(space, 'event-sources.json'))).find(source => source.kind === 'user_prompt');
  assert(seed?.context?.unlinked_reason === null, 'synthetic branch was not attributed');
  const now = Math.floor(Date.now() / 1000), midnight = Math.floor((now + 32400) / 86400) * 86400 - 32400;
  const at = age => Math.max(midnight, now - age);
  const texts = {
    decision: '검색 조건을 바꿔도 선택한 필터를 유지하기로 했습니다.',
    completion: '필터 유지 동작을 구현하고 관련 테스트 12개가 통과했다고 보고했습니다.',
    progress: '작은 화면에서 필터 목록의 줄바꿈을 확인하고 있습니다.',
    action: '화면 확인이 끝나면 변경 내용을 PR에 정리합니다.',
    commentary: '필터 유지 동작을 반영했습니다. 지금은 작은 화면에서도 선택 상태와 결과 개수를 함께 읽을 수 있는지 확인하고 있습니다.',
  };
  const sources = {};
  const source = (seq, kind, age, turn) => sources[seq] = {...seed, uid: 'readme-demo-' + seq, order: seq, kind, at_unix_utc: at(age), turn};
  source(1, 'user_prompt', 1200, 'turn-decision');
  source(2, 'assistant_final', 600, 'turn-implementation');
  source(3, 'assistant_final', 300, 'turn-progress');
  source(4, 'user_prompt', 60, 'turn-demo');
  const proof = (seq, quote) => [{source: sources[seq], quote}];
  const record = (key, kind, stage, outcome, text, seq) => ({
    id: key, key, kind, stage, outcome,
    versions: [{operation: 'add', text, condition: null, at_unix_utc: sources[seq].at_unix_utc, order: seq, target: null, proof: proof(seq, text)}],
    replaced_by: null, withdrawn: false, follows: null, relation_order: 0, corrections: [],
  });
  const stateRow = (key, kind, text, owner, condition) => ({key, kind, text, owner, condition, status: 'active', at_unix_utc: at(300), order: 3, proof: proof(3, text), uncertain_by_omission: false});
  const branchDirectory = join(state, 'branches', seed.context.key);
  mkdirSync(branchDirectory, {recursive: true});
  put(join(branchDirectory, 'context.json'), seed.context);
  put(join(branchDirectory, 'journal.json'), {
    revision: 1, through_order: 3, through_unix_utc: at(300), updated_at_unix_utc: at(300),
    records: {D1: record('D1', 'decision', 'decision', 'confirmed', texts.decision, 1), C1: record('C1', 'completion', 'implementation', 'reported', texts.completion, 2)},
    states: {P1: stateRow('P1', 'progress', texts.progress, 'Codex', null), A1: stateRow('A1', 'action', texts.action, 'Codex', '화면 확인 후')}, gaps: [],
  });
  put(join(space, 'event-sources.json'), sources);
  writeFileSync(join(space, 'inbox.jsonl'), JSON.stringify({seq: 4, kind: 'user_prompt', session_id: 'thread-demo', turn_id: 'turn-demo', model: null, content: request, truncated: false, recorded_at_unix: at(60), cwd: repo}) + '\n');
  const current = {revision: 1, base_revision: null, source_seq: 3, updated_at_unix: at(300), summary: {}, repo: {available: false}, timing: {}};
  put(join(space, 'current.json'), current);
  put(join(space, 'selected-current.json'), {current, done: [1, 2, 3], display_seq: 3});
  put(join(space, 'cursor.json'), {last_processed_seq: 3});
  put(join(space, 'status.json'), {state: 'idle', message: '', updated_at_unix: at(300)});
  const task = json(join(space, 'task-links.json')), thread = task.threads[task.active_thread];
  thread.current_request = request; thread.latest_source_seq = 4; thread.next_refresh_at = now + 3600; task.revision++;
  put(join(space, 'task-links.json'), task);
  const live = json(join(space, 'live-turn.json'));
  Object.assign(live, {source_prompt_seq: 4, revision: 2, updated_at: now, ended: false, status: 'inProgress', request, commentary: [{id: 'demo-commentary', text: texts.commentary, at_unix: now}], plan: [], activities: [], truncated: false});
  put(join(space, 'live-turn.json'), live);
  let raw = '', frame = '';
  const columns = 80, rows = 30;
  panel = spawn('python3', [join(project, 'scripts/pty-ui-readability.py'), binary, String(columns), String(rows)], {env, stdio: ['pipe', 'pipe', 'pipe']});
  panel.stdout.setEncoding('utf8'); panel.stderr.setEncoding('utf8');
  panel.stdout.on('data', data => {
    raw += data;
    const candidate = raw.slice(raw.lastIndexOf('\x1b[2J'));
    const footer = candidate.lastIndexOf('\x1b[' + rows + ';1H');
    if (footer >= 0 && candidate.indexOf('\x1b[0m', footer) >= 0) frame = candidate;
  });
  panel.stderr.on('data', data => raw += data); panel.stdin.on('error', () => {});
  const deadline = Date.now() + 10000;
  while ((!frame || !plain(frame).includes('다음 행동')) && Date.now() < deadline) await pause(25);
  assert(frame, 'TUI did not render');
  const visible = plain(frame).replace(/\s/g, '');
  for (const token of ['유효 결정 1건', '오늘 완료 1건', '기록된 진행 1건', '검색 조건', '필터 유지 동작', 'PR에 정리']) assert(visible.includes(token.replace(/\s/g, '')), 'Missing demo content: ' + token);
  assert(!visible.includes('요약 처리 실패') && !visible.includes('자동 재시도 중지'), 'Demo unexpectedly showed a processing failure');
  assert(!existsSync(calls), 'Capture invoked a provider or opener');
  writeFileSync(join(output, 'panel.ansi'), frame);
  panel.stdin.write('q');
  const closeDeadline = Date.now() + 10000;
  while (panel.exitCode === null && Date.now() < closeDeadline) await pause(25);
  assert.equal(panel.exitCode, 0, raw);
  const render = spawnSync('python3', [join(project, 'scripts/render-terminal-capture.py'), '--columns', String(columns), join(output, 'panel.ansi')], {env, encoding: 'utf8'});
  assert.equal(render.status, 0, render.stderr);
  const version = spawnSync(binary, ['--version'], {env, encoding: 'utf8'}).stdout.trim();
  const result = {passed: true, synthetic: true, version, columns, rows, binary_sha256: digest(binary), script_sha256: digest(new URL(import.meta.url)), png_sha256: digest(join(output, 'panel.png')), external_calls: 0, fixture_root: root, source: 'Captured current TUI; invented records and public commentary; no display text rewritten.'};
  writeFileSync(join(evidence, 'result.json'), JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify({passed: true, version, image: join(output, 'panel.png'), external_calls: 0}));
} finally {
  if (panel?.exitCode === null) panel.kill();
  guard?.stdin.end();
  if (spool) {
    rmSync(spool, {recursive: true, force: true});
  }
}
