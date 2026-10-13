// Synthetic rejected responses only; does not claim to replay a deleted live failure.
import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync,writeFileSync,readdirSync,existsSync,statSync,rmSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const root=mkdtempSync(join(tmpdir(),'pm-rejected-response-'));
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:root,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_WORKSPACE_ID:'fixture',HERDR_SESSION:'diagnostic-test',
HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),PM_TEST_ROOT:root,PM_TEST_BINARY:binary,MAEKLOG_CODEX_BIN:resolve('scripts/mock-branch-reducer.mjs'),
PM_TEST_CALLS:join(root,'calls.jsonl'),PM_TEST_DELTAS:join(root,'deltas.json'),TZ:'Asia/Seoul'};
delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_DISABLE_LUNA;
const run=(args,input)=>spawnSync(binary,args,{env,input,encoding:'utf8',timeout:15000});
const put=(p,v)=>writeFileSync(p,JSON.stringify(v));
const json=p=>JSON.parse(readFileSync(p,'utf8'));
try {
  assert.equal(run(['hook'],JSON.stringify({hook_event_name:'SessionStart',session_id:'fixture-session',cwd:root})).status,0);
  const key=readdirSync(join(root,'spaces'))[0],space=join(root,'spaces',key),at=Math.floor(Date.now()/1000);
  const events=[{seq:1,kind:'user_prompt',content:'문구를 수정해 줘'},{seq:2,kind:'assistant_final',content:'문구 수정\n완료: **강조**와 [링크](https://example.invalid)를 보존했습니다.'}].map(e=>({...e,session_id:'fixture-session',turn_id:'turn-1',model:null,cwd:root,truncated:false,recorded_at_unix:at}));
  const inbox=events.map(JSON.stringify).join('\n')+'\n';
  writeFileSync(join(space,'inbox.jsonl'),inbox);put(join(space,'sequence.json'),{next_seq:3});
  const definition=quote=>({2:{records:[{kind:'completion',operation:'add',key:'wording',target:null,text:'문구 수정 완료',condition:null,stage:'implementation',outcome:'reported',evidence:[{reference:'event:2',quote}]}]}});
  for(const [quote,code] of [['문구 수정 완료','quote_not_in_event'],['','empty_quote'],['가'.repeat(601),'quote_too_long']]) {
    put(env.PM_TEST_DELTAS,definition(quote));put(join(space,'daily-retries.json'),{});put(join(space,'retry.json'),{key:'',attempts:0,next_at:0});
    const failed=run(['worker-space',key]);assert.equal(failed.status,0,failed.stderr);
    assert(existsSync(join(space,'reducer-failure.json')),failed.stderr);
    assert(json(join(space,'status.json')).message.includes(code));
    const failure=json(join(space,'reducer-failure.json'));
    assert.equal(failure.stage,'journal_validation');assert(failure.detail.includes(code));
    assert.equal(failure.payload_omitted,false);assert.deepEqual(failure.model_events,events);assert.deepEqual(failure.original_events,events);
    assert.equal(JSON.parse(failure.response_utf8).record_updates[0].evidence[0].quote,quote);
    assert.equal(statSync(join(space,'reducer-failure.json')).mode&0o777,0o600);
    assert.equal(readFileSync(join(space,'inbox.jsonl'),'utf8'),inbox);
    assert(!existsSync(join(space,'selected-current.json')));
    assert(!readdirSync(join(root,'runtime')).some(n=>n.startsWith('luna-output-')));
  }
  for(const selector of [
    {reference:'event:2',candidate_id:'c45'},
    {reference:'event:999',candidate_id:'c1'},
    {reference:'event:2',segment_start:1,segment_end:999},
  ]) {
    const delta=definition('unused');delta[2].records[0].evidence=[selector];
    put(env.PM_TEST_DELTAS,delta);put(join(space,'daily-retries.json'),{});put(join(space,'retry.json'),{key:'',attempts:0,next_at:0});
    assert.equal(run(['worker-space',key]).status,0);
    assert.equal(json(join(space,'reducer-failure.json')).stage,'evidence_selection');
    assert.equal(readFileSync(join(space,'inbox.jsonl'),'utf8'),inbox);
    assert(!existsSync(join(space,'selected-current.json')));
  }
  // Exact newline quote succeeds without weakening verification or another repair call.
  put(env.PM_TEST_DELTAS,definition(events[1].content));put(join(space,'daily-retries.json'),{});put(join(space,'retry.json'),{key:'',attempts:0,next_at:0});
  assert.equal(run(['worker-space',key]).status,0);
  assert.equal(readFileSync(join(space,'inbox.jsonl'),'utf8').trim(),'');
  assert.equal(readFileSync(env.PM_TEST_CALLS,'utf8').trim().split('\n').length,7);
  assert(existsSync(join(space,'reducer-failure.json')),'latest failure must remain inspectable after success');
  assert.equal(run(['action','clear-records']).status,0);
  assert(!existsSync(join(space,'reducer-failure.json')),'Space clear must delete private failure payload');
  console.log('PASS synthetic diagnostics: three distinct quote failures, exact input/response capture, private mode, no invalid commit, correct newline quote succeeds, no extra model calls, scoped clear');
} finally { rmSync(root,{recursive:true,force:true}); }
