// Deterministic model outputs: validates admission/retry contracts, NOT model classification quality.
import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
const binary=resolve(process.argv[2]??'target/debug/maeklog'),root=mkdtempSync(join(tmpdir(),'pm-decision-contract-'));
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:root,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_PLUGIN_CONFIG_DIR:join(root,'config'),HERDR_WORKSPACE_ID:'fixture',HERDR_SESSION:'decision-test',HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),PM_TEST_ROOT:root,PM_TEST_BINARY:binary,MAEKLOG_CODEX_BIN:resolve('scripts/mock-branch-reducer.mjs'),PM_TEST_CALLS:join(root,'calls.jsonl'),PM_TEST_DELTAS:join(root,'deltas.json'),TZ:'Asia/Seoul'};
delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_DISABLE_LUNA;
const run=(args,input)=>spawnSync(binary,args,{env,input,encoding:'utf8',timeout:15000});
const ok=(args,input)=>{const r=run(args,input);assert.equal(r.status,0,r.stderr);return r.stdout;};
const put=(p,v)=>writeFileSync(p,JSON.stringify(v));
const json=p=>JSON.parse(readFileSync(p,'utf8'));
const calls=()=>existsSync(env.PM_TEST_CALLS)?readFileSync(env.PM_TEST_CALLS,'utf8').trim().split('\n').map(JSON.parse):[];
try {
  ok(['hook'],JSON.stringify({hook_event_name:'SessionStart',session_id:'fixture-session',cwd:root}));
  const key=readdirSync(join(root,'spaces'))[0],space=join(root,'spaces',key),at=Math.floor(Date.now()/1000);
  const make=(seq,content)=>({seq,kind:seq%2?'user_prompt':'assistant_final',content,session_id:'fixture-session',turn_id:'turn-'+Math.ceil(seq/2),model:null,cwd:root,truncated:false,recorded_at_unix:at+seq});
  const stage=(events)=>{writeFileSync(join(space,'inbox.jsonl'),events.map(JSON.stringify).join('\n')+'\n');put(join(space,'sequence.json'),{next_seq:events.at(-1).seq+1});};
  const evidence=(seq,quote)=>[{reference:'event:'+seq,quote}];
  const decision=(seq,text,basis)=>({kind:'decision',operation:'add',key:'decision-'+seq,target:null,text,condition:null,stage:'decision',outcome:'confirmed',evidence:evidence(seq,text),decision_basis:basis});
  const retryNow=()=>{const p=join(space,'daily-retries.json'),all=json(p);for(const b of Object.values(all))b.next_at=0;put(p,all);};
  // Reduced observed shape: missing-control question must not enter durable decisions
  // merely by deleting the old punctuation heuristic. A basis is required.
  const first=[make(1,'시점별 버튼은 어디갔어?'),make(2,'누락 상태를 확인하겠습니다.')];stage(first);
  put(env.PM_TEST_DELTAS,{1:{records:[decision(1,first[0].content,null)]}});
  ok(['worker-space',key]);assert(json(join(space,'status.json')).message.includes('decision_basis_required'));
  assert(!existsSync(join(space,'selected-current.json')));
  assert.equal(calls().length,1);assert.equal(calls()[0].feedback,false);assert.equal(calls()[0].validation_feedback,null);
  const originalInbox=readFileSync(join(space,'inbox.jsonl'),'utf8');
  retryNow();put(env.PM_TEST_DELTAS,{1:{states:[{key:'missing-controls',kind:'action',text:'시점별 버튼의 누락 여부를 확인한다',owner:'codex',condition:null,status:'active',change:'changed'}]}});
  ok(['worker-space',key]);assert.equal(calls().length,2);assert.equal(calls()[1].feedback,true);assert(calls()[1].validation_feedback.startsWith('decision_basis_required:'));
  assert.notEqual(readFileSync(join(space,'inbox.jsonl'),'utf8'),originalInbox);
  const branches=()=>readdirSync(join(root,'branches')).map(k=>json(join(root,'branches',k,'journal.json')));
  assert(branches().every(j=>Object.keys(j.records).length===0));
  assert(branches().some(j=>Object.values(j.states).some(s=>s.text.includes('누락 여부'))));
  // Explicit confirmation is valid even with a question elsewhere in the source.
  const affirmative='시점 버튼을 유지하기로 확정한다.';
  const second=[make(3,'버튼은 어디갔어? '+affirmative),make(4,'확인했습니다.')];stage(second);
  const confirmed=decision(3,second[0].content,{intent:'explicit_choice',reference:'event:3',quote:affirmative});
  confirmed.text='시점 버튼을 유지한다';
  put(env.PM_TEST_DELTAS,{3:{records:[confirmed]}});ok(['worker-space',key]);
  assert.equal(calls().at(-1).feedback,false,'unrelated work inherited failure feedback');
  assert.equal(calls().at(-1).validation_feedback,null,'unrelated work inherited the rejected batch diagnostic');
  assert(branches().some(j=>Object.values(j.records).some(r=>r.versions[0].decision_basis?.quote===affirmative)));
  // Same structural rejection twice stops BEFORE a third model invocation.
  const third=[make(5,'다른 버튼도 복원해 줘'),make(6,'확인하겠습니다.')];stage(third);
  put(env.PM_TEST_DELTAS,{5:{records:[decision(5,third[0].content,{intent:'request',reference:'event:5',quote:third[0].content})]}});
  ok(['worker-space',key]);retryNow();ok(['worker-space',key]);
  assert.equal(calls().length,5);assert.equal(calls().at(-1).feedback,true);
  const budget=json(join(space,'daily-retries.json'))['daily:today:6'];
  assert.equal(budget.attempts,2);assert.equal(budget.repeated_validation,2);
  retryNow();ok(['worker-space',key]);assert.equal(calls().length,5,'third repeated-invalid model call');
  assert(ok(['render-once']).replaceAll('\n','').includes('자동 재시도 중지'));
  assert(readFileSync(join(space,'inbox.jsonl'),'utf8').includes('다른 버튼'));
  // Reduced observed failure: one passage contains a temporary explicit choice
  // and a neighboring proposal. A good decision before a malformed one must not
  // leak into the journal. This fake output tests guards, not model comprehension.
  const mixed='언어 전환 대신 시스템 언어를 따르기로 하자. 일단은.\n모델 선택 기능이 필요할 듯해.';
  const fourth=[make(7,mixed),make(8,'요구사항을 확인했습니다.')];stage(fourth);
  const language=decision(7,mixed,{intent:'explicit_choice',reference:'event:7',quote:mixed});
  language.key='system-locale';language.text='언어는 시스템 언어를 따른다';language.condition='일단은';
  const proposed=decision(7,mixed,{intent:'explicit_choice',reference:'event:7',quote:mixed});
  proposed.key='model-selection';proposed.text='요약 모델을 선택할 수 있게 한다';proposed.stage='other';
  const completion={kind:'completion',operation:'add',key:'requirement-review',target:null,text:'요구사항 확인',condition:null,stage:'other',outcome:'reported',evidence:evidence(8,fourth[1].content),decision_basis:null};
  const journalBytes=()=>Object.fromEntries(readdirSync(join(root,'branches')).map(k=>[k,readFileSync(join(root,'branches',k,'journal.json'),'utf8')]));
  const beforeJournal=journalBytes(),beforeCurrent=readFileSync(join(space,'selected-current.json'),'utf8');
  const fourthInbox=readFileSync(join(space,'inbox.jsonl'),'utf8');
  put(env.PM_TEST_DELTAS,{7:{records:[language,proposed]},8:{records:[completion]}});
  ok(['worker-space',key]);assert.equal(calls().length,6);
  assert(json(join(space,'status.json')).message.includes('decision_stage_required:'));
  assert.equal(calls().at(-1).validation_feedback,null,'new batch inherited old request rejection');
  assert.deepEqual(journalBytes(),beforeJournal,'valid first decision partially committed before malformed second decision');
  assert.equal(readFileSync(join(space,'selected-current.json'),'utf8'),beforeCurrent);
  assert.equal(readFileSync(join(space,'inbox.jsonl'),'utf8'),fourthInbox);
  assert.equal(json(join(space,'reducer-failure.json')).stage,'journal_validation');
  // Correcting the tuple still cannot admit a classified proposal. Different
  // failures receive distinct feedback and do not trip the identical-error cap.
  retryNow();proposed.stage='decision';proposed.decision_basis.intent='proposal';
  put(env.PM_TEST_DELTAS,{7:{records:[language,proposed]},8:{records:[completion]}});
  ok(['worker-space',key]);assert.equal(calls().length,7);
  assert(calls().at(-1).validation_feedback.startsWith('decision_stage_required:'));
  assert(json(join(space,'status.json')).message.includes('decision_basis_not_confirmed:'));
  assert.deepEqual(journalBytes(),beforeJournal,'classified proposal was admitted after its tuple changed');
  assert.equal(readFileSync(join(space,'selected-current.json'),'utf8'),beforeCurrent);
  const different=json(join(space,'daily-retries.json'))['daily:today:8'];
  assert.equal(different.attempts,2);assert.equal(different.repeated_validation,1);
  // A corrected response keeps only the evidenced temporary choice, places the
  // proposal in current action state, and preserves the source anchor/time.
  retryNow();
  const modelAction={key:'model-selection',kind:'action',text:'모델 선택 요구사항을 검토한다',owner:'codex',condition:null,status:'active',change:'changed',evidence:evidence(7,mixed)};
  put(env.PM_TEST_DELTAS,{7:{records:[language],states:[modelAction]},8:{records:[completion]}});
  ok(['worker-space',key]);assert.equal(calls().length,8);
  assert(calls().at(-1).validation_feedback.startsWith('decision_basis_not_confirmed:'));
  assert.equal(readFileSync(join(space,'inbox.jsonl'),'utf8').trim(),'');
  const storedLanguage=branches().flatMap(j=>Object.values(j.records)).find(r=>r.key==='system-locale');
  assert(storedLanguage);assert.equal(storedLanguage.stage,'decision');assert.equal(storedLanguage.outcome,'confirmed');
  assert.equal(storedLanguage.versions[0].condition,'일단은');
  assert.equal(storedLanguage.versions[0].decision_basis.quote,mixed);
  assert.equal(storedLanguage.versions[0].proof[0].quote,mixed);
  assert.equal(storedLanguage.versions[0].proof[0].source.kind,'user_prompt');
  assert.equal(storedLanguage.versions[0].at_unix_utc,fourth[0].recorded_at_unix);
  assert(branches().every(j=>!Object.values(j.records).some(r=>r.key==='model-selection')));
  assert(branches().some(j=>Object.values(j.states).some(s=>s.key==='model-selection'&&s.kind==='action')));
  // The same shape error twice remains capped at two model calls. A later new
  // batch gets no old diagnostic, and no automatic third correction is spawned.
  const fifth=[make(9,'그 방안으로 확정한다.'),make(10,'확인했습니다.')];stage(fifth);
  const invalidStage=decision(9,fifth[0].content,{intent:'explicit_choice',reference:'event:9',quote:fifth[0].content});
  invalidStage.stage='other';put(env.PM_TEST_DELTAS,{9:{records:[invalidStage]}});
  ok(['worker-space',key]);assert.equal(calls().length,9);assert.equal(calls().at(-1).validation_feedback,null);
  retryNow();ok(['worker-space',key]);assert.equal(calls().length,10);
  assert(calls().at(-1).validation_feedback.startsWith('decision_stage_required:'));
  const same=json(join(space,'daily-retries.json'))['daily:today:10'];
  assert.equal(same.attempts,2);assert.equal(same.repeated_validation,2);
  retryNow();ok(['worker-space',key]);assert.equal(calls().length,10,'third identical stage-error model call');
  assert.equal(readFileSync(join(space,'inbox.jsonl'),'utf8').trim(),fifth.map(JSON.stringify).join('\n'));
  assert(calls().every(c=>c.decision_tuple_contract&&c.mixed_claim_contract),'invoked prompt omitted bounded decision contract');
  assert(calls().every(c=>c.args.some(a=>a.endsWith('context-v6.schema.json'))&&c.args.includes('model_reasoning_effort="medium"')&&c.args.includes('service_tier="default"')&&c.args.includes('features.fast_mode=false')));
  console.log('PASS decision contract: request/proposal rejected, explicit choice preserved with source/temporary condition, bad tuple rejects whole batch, precise distinct retry feedback, identical-error two-attempt stop, no cross-work feedback; fake provider only');
} finally {rmSync(root,{recursive:true,force:true});}
