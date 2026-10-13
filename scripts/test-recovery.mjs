// Run with: node scripts/test-recovery.mjs /absolute/path/to/maeklog
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, writeFileSync, readFileSync, chmodSync, rmSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {resolve, join} from 'node:path';
import {tmpdir} from 'node:os';

const binary = resolve(process.argv[2] ?? 'target/debug/maeklog');
const root = mkdtempSync(join(tmpdir(), 'pm-recovery-integration-'));
const space = join(root, 'spaces', 'test-space');
mkdirSync(space, {recursive:true, mode:0o700});
const put = (name, value) => writeFileSync(join(space, name), JSON.stringify(value), {mode:0o600});
const get = name => JSON.parse(readFileSync(join(space, name), 'utf8'));
const mock = resolve('scripts/mock-reducer.mjs');
chmodSync(mock, 0o755);
const env = {...process.env,LC_ALL:'ko_KR.UTF-8', MAEKLOG_STATE_DIR:root, MAEKLOG_PLUGIN_ROOT:process.cwd(),
  MAEKLOG_CODEX_BIN:mock, PM_TEST_CALLS:join(root,'calls.jsonl'), PM_TEST_FAIL:'0'};
delete env.MAEKLOG_DISABLE_LUNA;
delete env.MAEKLOG_INTERNAL;
const worker = overrides => {
  const result = spawnSync(binary, ['worker'], {env:{...env,...overrides},encoding:'utf8',timeout:15000});
  assert.equal(result.status, 0, result.stderr);
};
const calls = () => readFileSync(env.PM_TEST_CALLS,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse);
try {
  put('meta.json',{space_key:'test-space',workspace_id:'test',session_name:'default',cwd:root,label:'test',created_at_unix:1,last_seen_at_unix:1,archived:false});
  put('current.json',{revision:1,base_revision:0,source_seq:0,updated_at_unix:1,summary:{objective:'테스트 프로젝트 목표'}});
  const events = Array.from({length:21}, (_,i) => ({seq:i+1,kind:i%2 ? 'assistant_final':'user_prompt',session_id:'old',content:'test',truncated:false,recorded_at_unix:Math.floor(Date.now()/1000),cwd:root}));
  writeFileSync(join(space,'inbox.jsonl'), events.map(e=>JSON.stringify(e)).join('\n')+'\n');
  put('sequence.json',{next_seq:22});
  worker();
  assert.deepEqual(calls(), [[13,14,15,16,17,18,19,20],[5,6,7,8,9,10,11,12],[1,2,3,4]]);
  assert.equal(get('current.json').source_seq,20);
  assert.equal(get('current.json').revision,4);
  const processing=get('processing.json');
  assert.equal(processing[0].event_count,8);
  assert.equal(processing[0].outcome,'success');
  assert.equal(processing[0].event_chars,32);
  assert.equal(typeof processing[0].elapsed_ms,'number');
  assert.equal(JSON.stringify(processing).includes('테스트 프로젝트 목표'),false);
  assert.equal(JSON.parse(readFileSync(join(space,'inbox.jsonl'),'utf8')).seq,21);

  // Simulate a third failed attempt: preserve the exact cached summary and raw queue.
  writeFileSync(join(space,'inbox.jsonl'),events.slice(18,20).map(e=>JSON.stringify({...e,seq:e.seq+2})).join('\n')+'\n');
  put('retry.json',{key:'daily:today:22',attempts:2,next_at:0});
  put('latest-retry.json',{key:'latest:22',attempts:3,next_at:0});
  const before = readFileSync(join(space,'current.json'),'utf8');
  worker({PM_TEST_FAIL:'1'});
  assert.equal(get('retry.json').attempts,3);
  assert.equal(get('status.json').state,'error');
  assert.equal(readFileSync(join(space,'current.json'),'utf8'),before);
  assert.equal(calls().length,4);
  assert.equal(get('processing.json').at(-1).outcome,'failed');
  assert.equal(get('processing.json').at(-1).event_count,2);
  worker({PM_TEST_FAIL:'1'}); // restarting must not reset the budget
  assert.equal(calls().length,4);
  put('daily-retries.json',{});put('retry.json',{key:'',attempts:0,next_at:0}); // explicit reset, as with panel r
  worker();
  assert.equal(get('current.json').source_seq,22);

  // A scheduling disagreement must not block the entire brief or invent readiness.
  writeFileSync(join(space,'inbox.jsonl'),events.slice(0,2).map(e=>JSON.stringify({...e,seq:e.seq+22})).join('\n')+'\n');
  worker({PM_TEST_TIMING_CONFLICT:'1'});
  assert.equal(get('current.json').source_seq,24);
  assert.equal(get('current.json').summary.objective,'');
  assert.equal(get('current.json').summary.decisions[0].text,'보존할 정상 결정');
  assert.equal(get('current.json').summary.progress[0].text,'보존할 정상 진행');
  const action=get('current.json').summary.next_actions[0];
  assert.equal(action.text,'보존할 구현 행동');
  assert.equal(action.step,null);
  assert.equal(action.when,null);
  const diagnostics=JSON.parse(readFileSync(join(root,'runtime/diagnostics/action-timing.json')));
  assert.equal(diagnostics.at(-1).last_event_seq,24);
  assert.deepEqual(diagnostics.at(-1).conflicts,[{action_index:0,step:0,when:'승인 후'}]);
  // Other invalid fields still reject the whole response and preserve the cursor/queue.
  writeFileSync(join(space,'inbox.jsonl'),events.slice(0,2).map(e=>JSON.stringify({...e,seq:e.seq+24})).join('\n')+'\n');
  put('retry.json',{key:'daily:today:26',attempts:2,next_at:0});
  put('latest-retry.json',{key:'latest:26',attempts:3,next_at:0});
  worker({PM_TEST_TIMING_CONFLICT:'1',PM_TEST_BAD_OWNER:'1'});
  assert.equal(get('current.json').source_seq,24);
  assert.equal(get('status.json').state,'error');
  assert.equal(readFileSync(join(space,'inbox.jsonl'),'utf8').trim().split('\n').length,2);

  // Real hook entry point, fake space only: new session connects without reducing/resetting memory.
  const sessionRoot=join(root,'sessions');
  const hook = input => spawnSync(binary,['hook'],{env:{...env,MAEKLOG_STATE_DIR:sessionRoot,HERDR_WORKSPACE_ID:'test',HERDR_SESSION:'default'},input:JSON.stringify({...input,cwd:root}),encoding:'utf8',timeout:5000});
  assert.equal(hook({hook_event_name:'UserPromptSubmit',session_id:'old',prompt:'still open'}).stdout.trim(),'{}');
  assert.equal(hook({hook_event_name:'SessionStart',session_id:'new',source:'clear'}).stdout.trim(),'{}');
  // The test hook must not spawn a model for an unfinished prompt or a session-start alone.
  const {readdirSync} = await import('node:fs');
  const directory=join(sessionRoot,'spaces',readdirSync(join(sessionRoot,'spaces'))[0]);
  assert.equal(JSON.parse(readFileSync(join(directory,'session.json'))).session_id,'new');
  const started=JSON.parse(readFileSync(join(directory,'connection.json')));
  assert.equal(started.session_start_id,'new');
  assert.equal(typeof started.session_start_at,'number');
  assert.equal(hook({hook_event_name:'SessionStart',session_id:'compact-only',source:'compact'}).stdout.trim(),'{}');
  assert.equal(JSON.parse(readFileSync(join(directory,'connection.json'))).session_start_id,'new');
  assert.equal(readFileSync(join(directory,'inbox.jsonl'),'utf8').trim().split('\n').length,1);
  assert.equal(calls().length,7);
  console.log('PASS: bounded batches, open-turn retention, durable retry cap, cached-summary retention, recovery, SessionStart hook, action-timing quarantine and diagnostics, unrelated validation retained');
} finally {
  rmSync(root,{recursive:true,force:true});
}
