// Full hook -> tab store -> model fixture -> render/action pipeline, no real model or Herdr.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,readdirSync,rmSync,chmodSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
const binary=resolve(process.argv[2]??'target/debug/maeklog'),root=mkdtempSync(join(tmpdir(),'pm-tabs-')),state=join(root,'state');
const config=join(root,'tabs.json'),json=p=>JSON.parse(readFileSync(p,'utf8')),put=(p,v)=>writeFileSync(p,JSON.stringify(v));
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_WORKSPACE_ID:'fixture',HERDR_SESSION:'tabs-test',PM_TEST_ROOT:root,PM_TEST_BINARY:binary,HERDR_BIN_PATH:resolve('scripts/mock-tab-herdr.mjs'),MAEKLOG_CODEX_BIN:resolve('scripts/mock-branch-reducer.mjs'),PM_TEST_DELTAS:join(root,'deltas.json'),PM_TEST_CALLS:join(root,'calls.jsonl'),TZ:'Asia/Seoul'};
delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_DISABLE_LUNA;
chmodSync(env.HERDR_BIN_PATH,0o755);
const run=(tab,args,input,extra={})=>spawnSync(binary,args,{env:{...env,HERDR_TAB_ID:tab,HERDR_PANE_ID:tab.replace(':t',':p'),...extra},input:input?JSON.stringify(input):undefined,encoding:'utf8',timeout:20000});
const ok=(...a)=>{const r=run(...a);assert.equal(r.status,0,r.stderr);assert(!r.stderr.includes('maeklog hook:'),r.stderr);return r.stdout;};
const git=(...a)=>{const r=spawnSync('git',a,{cwd:root,encoding:'utf8'});assert.equal(r.status,0,r.stderr);};
mkdirSync(state,{recursive:true});
const guard=spawn('python3',['-c','import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()',join(state,'worker.lock')],{stdio:['pipe','pipe','inherit']});
await new Promise((resolve,reject)=>{guard.stdout.once('data',resolve);guard.once('error',reject);});
try {
 git('init','-b','main');git('-c','user.name=Fixture','-c','user.email=f@example.invalid','commit','--allow-empty','-m','fixture');
 put(config,{tabs:['fixture:t1','fixture:t2'],active:'fixture:t1',panels:{}});put(env.PM_TEST_DELTAS,{});
 // Opening tab two must not return tab one's panel even while tab one is focused.
 for(const tab of ['fixture:t1','fixture:t2'])ok(tab,['action','open']);
 let topology=json(config);assert.equal(Object.keys(topology.panels).length,2);assert.notEqual(topology.panels['fixture:t1'].pane_id,topology.panels['fixture:t2'].pane_id);
 ok('fixture:t2',['action','open']);assert.equal(Object.keys(json(config).panels).length,2,'duplicate panel');
 const stores=()=>readdirSync(join(state,'spaces')).map(key=>({key,meta:json(join(state,'spaces',key,'meta.json'))}));
 const a=stores().find(s=>s.meta.tab_id==='fixture:t1'),b=stores().find(s=>s.meta.tab_id==='fixture:t2');assert(a&&b);assert.notEqual(a.key,b.key);
 // Collect prompts through real hooks; seed final events without spawning a coordinator.
 for(const [tab,text] of [['fixture:t1','탭 A만 변경 확정'],['fixture:t2','탭 B만 유지 확정']]) {
  ok(tab,['hook'],{hook_event_name:'UserPromptSubmit',session_id:'session-'+tab,turn_id:'turn-1',prompt:text,cwd:root});
  const store=stores().find(s=>s.meta.tab_id===tab),dir=join(state,'spaces',store.key);
  const e=JSON.parse(readFileSync(join(dir,'inbox.jsonl'),'utf8').trim());
  // The coordinator lock keeps hook-triggered workers idle; explicitly run each
  // tab's worker below using its own deterministic fixture response.
  ok(tab,['hook'],{hook_event_name:'Stop',session_id:'session-'+tab,turn_id:'turn-1',last_assistant_message:'확인했습니다',cwd:root});
  assert.equal(e.content,text);
 }
 for(const [tab,store,text] of [['fixture:t1',a,'탭 A만 변경 확정'],['fixture:t2',b,'탭 B만 유지 확정']]) {
  put(env.PM_TEST_DELTAS,{1:{records:[{kind:'decision',operation:'add',key:'same-key',target:null,text,condition:null,stage:'decision',outcome:'confirmed'}],states:[{key:'next',kind:'action',text:text+' 후속',owner:'user',condition:null,status:'active',change:'changed'}]}});
  put(join(state,'spaces',store.key,'daily-retries.json'),{});
  ok(tab,['worker-space',store.key]);
 }
 const renderA=ok('fixture:t1',['render-once']).replaceAll('\n',''),renderB=ok('fixture:t2',['render-once']).replaceAll('\n','');
 assert(renderA.includes('탭 A만 변경 확정'));assert(!renderA.includes('탭 B만 유지 확정'));
 assert(renderB.includes('탭 B만 유지 확정'));assert(!renderB.includes('탭 A만 변경 확정'));
 assert(renderA.includes('이 탭의 저장 기록'));assert(renderB.includes('이 탭의 저장 기록'));
 assert.equal(json(join(state,'spaces',a.key,'meta.json')).tab_id,'fixture:t1');
 assert.equal(json(join(state,'spaces',b.key,'meta.json')).tab_id,'fixture:t2');
 // Hiding/disabling one tab cannot close the peer or change its participation.
 ok('fixture:t1',['action','hide']);assert(!json(config).panels['fixture:t1']);assert(json(config).panels['fixture:t2']);
 ok('fixture:t1',['action','open']);ok('fixture:t1',['action','disable']);
 assert.equal(json(join(state,'spaces',a.key,'participation.json')).enabled,false);
 assert.ok(!json(join(state,'spaces',b.key,'meta.json')).archived);
 const before=readFileSync(join(state,'spaces',a.key,'inbox.jsonl'),'utf8');
 ok('fixture:t1',['hook'],{hook_event_name:'UserPromptSubmit',session_id:'session-fixture:t1',turn_id:'turn-2',prompt:'수집 금지',cwd:root});
 assert.equal(readFileSync(join(state,'spaces',a.key,'inbox.jsonl'),'utf8'),before);
 ok('fixture:t2',['action','startup']);assert(!json(config).panels['fixture:t1']);assert(json(config).panels['fixture:t2']);
 // Live pane identity beats inherited tab after a move.
 topology=json(config);topology.moved='fixture:p2';put(config,topology);
 ok('fixture:t1',['hook'],{hook_event_name:'UserPromptSubmit',session_id:'moved-session',turn_id:'turn-3',prompt:'이동한 탭의 대화',cwd:root});
 assert(readFileSync(join(state,'spaces',b.key,'inbox.jsonl'),'utf8').includes('이동한 탭의 대화'));
 assert(!readFileSync(join(state,'spaces',a.key,'inbox.jsonl'),'utf8').includes('이동한 탭의 대화'));
 console.log('PASS tabs: two panels, idempotent open, tab-only hook/branch state/render, isolated hide/disable/restart, moved-pane routing');
} finally {guard.stdin.end();await new Promise(resolve=>guard.once("exit",resolve));rmSync(root,{recursive:true,force:true});}
