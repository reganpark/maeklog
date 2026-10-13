// Real CLI + frozen manual model fixture + isolated terminal scope; no external services.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync,chmodSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const root=mkdtempSync(join(tmpdir(),'pm-live-progress-')),state=join(root,'state'),topology=join(root,'topology.json'),calls=join(root,'calls.jsonl'),gate=join(root,'gate');
const put=(p,v)=>writeFileSync(p,JSON.stringify(v)),json=p=>JSON.parse(readFileSync(p,'utf8'));
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_ENV:'1',HERDR_WORKSPACE_ID:'wA',HERDR_TAB_ID:'wA:t1',HERDR_PANE_ID:'wA:p1',HERDR_SESSION:'live-test',HERDR_SOCKET_PATH:'/fixture/live.sock',HERDR_BIN_PATH:resolve('scripts/mock-live-herdr.mjs'),PM_CLIENT_TOPOLOGY:topology,MAEKLOG_CODEX_BIN:resolve('scripts/mock-live-summary.mjs'),PM_TEST_CALLS:calls,PM_TEST_GATE:gate};
delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_DISABLE_LUNA;
for(const p of [env.HERDR_BIN_PATH,env.MAEKLOG_CODEX_BIN])chmodSync(p,0o755);
mkdirSync(state);put(join(state,'client-routing.json'),{clients:[],turns:{}});
const pane=n=>({pane_id:`wA:p${n}`,workspace_id:'wA',tab_id:`wA:t${n}`,terminal_id:`terminal-${n}`,pid:100+n,agent:'codex',cwd:root});
put(topology,{socket:env.HERDR_SOCKET_PATH,panes:[pane(1),pane(2)]});
const run=(args,input,extra={})=>{const r=spawnSync(binary,args,{env:{...env,...extra},input:input?JSON.stringify(input):undefined,encoding:'utf8',timeout:12000});assert.equal(r.status,0,r.stderr);return r;};
const hook=(turn,event='UserPromptSubmit',cwd=root)=>run(['hook'],{session_id:'thread-a',turn_id:turn,hook_event_name:event,cwd,prompt:'수전 모델 제작과 렌더 검토',last_assistant_message:'작업 종료 보고'});
const stores=()=>readdirSync(join(state,'spaces')).map(k=>({key:k,dir:join(state,'spaces',k),meta:json(join(state,'spaces',k,'meta.json'))}));
const store=()=>stores().find(s=>s.meta.tab_id==='wA:t1');
const trace=()=>existsSync(calls)?readFileSync(calls,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
let sequence=0;
const live=(turn,method,fields={},cwd=root)=>run(['live','event','101','thread-a'],{sequence:++sequence,cwd,notification:{method,params:{threadId:'thread-a',...(['turn/started','turn/completed'].includes(method)?{turn:{id:turn,status:method==='turn/started'?'inProgress':'completed',...fields}}:{turnId:turn,...fields})}}});
const report=(turn,id,text,cwd=root)=>live(turn,'item/completed',{item:{type:'agentMessage',id,phase:'commentary',text}},cwd);
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const until=async(test,label)=>{for(let i=0;i<200;i++){if(test())return;await sleep(25);}throw new Error('Timed out: '+label);};
const capture=async(label,width,pressSummary=false)=>{
  const terminalEnv={...env,TERM:'xterm-256color'};delete terminalEnv.NO_COLOR;
  let output='',stderr='';
  const child=spawn('python3',[resolve('scripts/pty-live-panel.py'),binary,String(width),'36'],{env:terminalEnv,stdio:['pipe','pipe','pipe']});
  child.stdout.setEncoding('utf8');child.stdout.on('data',chunk=>output+=chunk);child.stderr.on('data',chunk=>stderr+=chunk.toString());
  const frame=()=>output.slice(output.lastIndexOf('\x1b[2J'));
  const visible=()=>frame().replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
  try {
    await until(()=>visible().includes('진행설명')&&visible().includes('응답진행중')&&(visible().includes('m메뉴')||visible().endsWith('·m')),'sized progress panel');
    if(pressSummary){child.stdin.write('s');await until(()=>visible().includes('추가호출없음'),'s shortcut reuses existing summary');}
    const callsBefore=trace().length;
    child.stdin.write('m');await until(()=>visible().includes('기능선택'),'progress menu did not render');
    child.stdin.write('\x1b[B\x1b[B\x1b[B\r');await until(()=>visible().includes('고정된내용')&&visible().includes('현재진행상세'),'full progress reader inaccessible');
    const category=async(check,label)=>{
      for(let n=0;n<10;n++){
        if(check())return;
        const prior=frame();child.stdin.write(']');await until(()=>frame()!==prior,'progress category did not change');
      }
      throw new Error(label);
    };
    if(pressSummary)await category(()=>visible().includes('여기까지요약·저장원장과별도'),'cached manual summary inaccessible');
    else await category(()=>visible().includes('실행중')&&visible().includes('Blender·render'),'collected running activity inaccessible');
    assert.equal(trace().length,callsBefore,'read-only full progress requested a model');
    await sleep(100);
    if(process.env.PM_LIVE_EVIDENCE_DIR){mkdirSync(process.env.PM_LIVE_EVIDENCE_DIR,{recursive:true});writeFileSync(join(process.env.PM_LIVE_EVIDENCE_DIR,label+'.ansi'),output);}
    child.stdin.write('\x1b');await until(()=>!visible().includes('고정된내용'),'progress reader Escape did not return');
    child.stdin.write('q');await until(()=>child.exitCode!==null,'terminal cleanup');
    assert.equal(child.exitCode,0,stderr);assert(output.includes('\x1b[?1006l'));
  } catch(error) {
    if(process.env.PM_LIVE_EVIDENCE_DIR){mkdirSync(process.env.PM_LIVE_EVIDENCE_DIR,{recursive:true});writeFileSync(join(process.env.PM_LIVE_EVIDENCE_DIR,label+'-failure.ansi'),output);}
    throw new Error(String(error)+'\n'+output.slice(-1200).replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'')+'\n'+stderr,{cause:error});
  } finally {if(child.exitCode===null){child.stdin.write('q');await sleep(300);if(child.exitCode===null)child.kill('SIGTERM');}}
};
const guard=spawn('python3',['-c','import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()',join(state,'worker.lock')],{stdio:['pipe','pipe','inherit']});
await new Promise((done,reject)=>{guard.stdout.once('data',done);guard.once('error',reject);});
let locked=true;
try {
  run(['client','attach','101','thread-a']);
  hook('t1');live('t1','turn/started');
  report('t1','r1','몸체를 생성했고 치수를 확인하고 있습니다');
  live('t1','turn/plan/updated',{plan:[{step:'몸체 제작',status:'completed'},{step:'렌더 확인',status:'inProgress'}]});
  live('t1','item/started',{item:{type:'mcpToolCall',id:'tool1',server:'Blender',tool:'render',status:'inProgress'}});
  const firstRender=run(['render-once']).stdout;
  assert.match(firstRender,/응답 진행 중/);assert.match(firstRender,/몸체/);
  assert(!firstRender.includes('작업 완료'),'started response looked complete');
  assert.equal(trace().length,0,'automatic progress must not call a model');
  await capture('panel-working',52);await capture('panel-narrow',28);
  assert.equal(trace().length,0,'opening and refreshing the live panel must not call a model');
  const dir=store().dir,baseInbox=readFileSync(join(dir,'inbox.jsonl'),'utf8');
  const rev=json(join(dir,'live-turn.json')).revision;
  report('t1','r1','몸체를 생성했고 치수를 확인하고 있습니다');
  assert.equal(json(join(dir,'live-turn.json')).revision,rev);
  assert.equal(readFileSync(join(dir,'inbox.jsonl'),'utf8'),baseInbox);
  // Requests freeze one snapshot and coalesce even while new observations arrive.
  writeFileSync(gate,'');run(['catch-up']);run(['catch-up']);
  const frozen=json(join(dir,'live-summary.json')).snapshot;
  report('t1','r2','두 번째 렌더를 확인하고 있습니다');run(['catch-up']);
  assert.deepEqual(json(join(dir,'live-summary.json')).snapshot,frozen);
  guard.stdin.end();await new Promise(done=>guard.once('exit',done));locked=false;
  const worker=spawn(binary,['worker'],{env,stdio:'ignore'});
  await until(()=>trace().some(c=>c.kind==='live'),'first explicit summary starts');
  report('t1','r3','새 기록은 다음 요청에서 정리합니다');run(['catch-up']);
  assert.equal(trace().filter(c=>c.kind==='live').length,1);
  rmSync(gate,{force:true});
  await until(()=>json(join(dir,'live-summary.json')).status==='ready','frozen summary saved');
  const rendered=run(['render-once']).stdout;
  assert.match(rendered,/현재 턴 잠정 요약 이후 새 활동 있음 · 미반영/);
  assert.equal(readFileSync(join(dir,'inbox.jsonl'),'utf8'),baseInbox);
  assert(!existsSync(join(dir,'selected-current.json')));
  // A new content version gets one new request; an unchanged ready version gets none.
  run(['catch-up']);
  await until(()=>trace().filter(c=>c.kind==='live').length===2&&json(join(dir,'live-summary.json')).status==='ready','new revision summarized');
  assert.match(run(['catch-up']).stdout,/추가 호출 없음/);
  await capture('panel-summary',52,true);
  await sleep(100);assert.equal(trace().filter(c=>c.kind==='live').length,2);
  for(const c of trace()) {
    assert(c.args.includes('gpt-6-luna'));assert(c.args.includes('model_reasoning_effort="medium"'));
    assert(c.args.includes('features.fast_mode=false'));assert(c.args.includes('features.hooks=false'));
  }
  // Wrong caller PID and child thread cannot write progress into this scope.
  const wrong=spawnSync(binary,['live','event','102','thread-a'],{env,encoding:'utf8',input:JSON.stringify({sequence:++sequence,cwd:root,notification:{method:'turn/started',params:{threadId:'thread-a',turn:{id:'t1'}}}})});
  assert.notEqual(wrong.status,0);assert.match(wrong.stderr,/does not own/);
  const child=spawnSync(binary,['live','event','101','thread-a'],{env,encoding:'utf8',input:JSON.stringify({sequence:++sequence,cwd:root,notification:{method:'turn/started',params:{threadId:'child-thread',turn:{id:'t1'}}}})});
  assert.notEqual(child.status,0);assert.match(child.stderr,/another thread/);
  hook('t1','Stop');
  await until(()=>existsSync(join(dir,'selected-current.json'))&&json(join(dir,'live-turn.json')).ended,'ordinary completed summary');
  assert.equal(trace().filter(c=>c.kind==='completed').length,1);
  assert.doesNotMatch(run(['render-once']).stdout,/현재 턴 잠정 요약/);
  await until(()=>json(join(dir,'selected-current.json')).done.includes(json(join(dir,'live-turn.json')).source_final_seq),'final committed');
  const firstEnded=readFileSync(join(dir,'live-turn.json'),'utf8');
  hook('t1','Stop');
  assert.equal(readFileSync(join(dir,'live-turn.json'),'utf8'),firstEnded,'duplicate final cannot create a phantom pending boundary');
  assert.doesNotMatch(run(['render-once']).stdout,/응답 종료 · 최종 정리 대기/);
  // End a later task while its manual model is running: no temporary result is published.
  hook('t2');live('t2','turn/started');report('t2','r4','렌더를 검토하고 있습니다');
  writeFileSync(gate,'');run(['catch-up']);
  await until(()=>trace().filter(c=>c.kind==='live').length===3,'third manual call starts');
  hook('t2','Stop');
  await until(()=>trace().filter(c=>c.kind==='completed').length===2,'final cancels manual work and runs existing summary');
  assert.notEqual(json(join(dir,'live-summary.json')).status,'ready');
  assert.doesNotMatch(run(['render-once']).stdout,/현재 턴 잠정 요약/);
  rmSync(gate,{force:true});
  report('t2','late','遅れて届いた進捗');
  assert(json(join(dir,'live-turn.json')).ended);
  // Disabling and re-enabling cannot resurrect an excluded in-flight task.
  hook('t3');live('t3','turn/started');report('t3','r5','新しい作業');
  put(join(dir,'participation.json'),{enabled:false,generation:1,history_blocked:true,open_turns:{}});
  const before=readFileSync(join(dir,'live-turn.json'),'utf8');report('t3','off','excluded');
  assert.equal(readFileSync(join(dir,'live-turn.json'),'utf8'),before);
  put(join(dir,'participation.json'),{enabled:true,generation:2,history_blocked:true,open_turns:{}});
  report('t3','reenabled-old','excluded');assert.equal(readFileSync(join(dir,'live-turn.json'),'utf8'),before);
  assert.doesNotMatch(run(['render-once']).stdout,/新しい作業/);
  await new Promise(done=>{if(worker.exitCode!==null)done();else worker.once('exit',done);});
  // A late queued event from a previous cwd must not retarget the current tab.
  const repo=name=>{
    const path=join(root,name);mkdirSync(path);
    for(const args of [['init','-q','-b','main',path],['-C',path,'-c','user.name=Fixture','-c','user.email=fixture@invalid','commit','-q','--allow-empty','-m','fixture']]){
      const r=spawnSync('git',args,{encoding:'utf8'});assert.equal(r.status,0,r.stderr);
    }
    return path;
  };
  const repoA=repo('repo-a'),repoB=repo('repo-b');
  const lateGuard=spawn('python3',['-c','import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()',join(state,'worker.lock')],{stdio:['pipe','pipe','inherit']});
  await new Promise((done,reject)=>{lateGuard.stdout.once('data',done);lateGuard.once('error',reject);});
  try {
    hook('scope-a','UserPromptSubmit',repoA);live('scope-a','turn/started',{},repoA);
    hook('scope-b','UserPromptSubmit',repoB);live('scope-b','turn/started',{},repoB);
    run(['catch-up']);
    const beforeMeta=readFileSync(join(dir,'meta.json'),'utf8');
    const beforeLive=readFileSync(join(dir,'live-turn.json'),'utf8');
    const beforeSummary=readFileSync(join(dir,'live-summary.json'),'utf8');
    report('scope-a','late-cwd','must stay in old task',repoA);
    live('scope-a','turn/plan/updated',{plan:[{step:'old',status:'inProgress'}]},repoA);
    live('scope-a','turn/completed',{},repoA);live('scope-a','turn/started',{},repoA);
    assert.equal(readFileSync(join(dir,'meta.json'),'utf8'),beforeMeta);
    assert.equal(readFileSync(join(dir,'live-turn.json'),'utf8'),beforeLive);
    assert.equal(readFileSync(join(dir,'live-summary.json'),'utf8'),beforeSummary);
    assert.match(run(['render-once']).stdout,/현재 턴 잠정 요약 · 요청한 시점 정리 중/);
    assert.equal(json(join(dir,'meta.json')).cwd,repoB);
    const inboxBeforeLateStop=readFileSync(join(dir,'inbox.jsonl'),'utf8');
    hook('scope-a','Stop',repoA);hook('scope-a','Interrupt',repoA);
    assert.equal(readFileSync(join(dir,'meta.json'),'utf8'),beforeMeta,'late terminal hooks must not retarget the current tab');
    assert.equal(readFileSync(join(dir,'inbox.jsonl'),'utf8'),inboxBeforeLateStop);
    // Accepted finals retain A's captured origin without changing B's current location.
    put(join(dir,'participation.json'),{enabled:true,generation:2,history_blocked:false,open_turns:{}});
    hook('scope-a','Stop',repoA);hook('scope-a','Interrupt',repoA);
    assert.equal(readFileSync(join(dir,'meta.json'),'utf8'),beforeMeta);
    assert.equal(readFileSync(join(dir,'live-turn.json'),'utf8'),beforeLive);
    assert.equal(readFileSync(join(dir,'live-summary.json'),'utf8'),beforeSummary);
    const sources=json(join(dir,'event-sources.json'));
    const latestFinal=readFileSync(join(dir,'inbox.jsonl'),'utf8').trim().split('\n').map(JSON.parse).filter(e=>e.turn_id==='scope-a'&&e.kind!=='user_prompt');
    assert.equal(latestFinal.length,2);
    assert(latestFinal.every(e=>sources[e.seq].context.worktree===repoA));
    assert.equal(trace().filter(c=>c.kind==='live').length,3);
    // A native attach-only switch arrives before the next session has a prompt.
    run(['client','attach','101','thread-b']);
    const switched=run(['render-once']).stdout;
    assert.doesNotMatch(switched,/응답 진행 중|현재 턴 잠정 요약|최근 요청/);
    const unavailable=spawnSync(binary,['catch-up'],{env,encoding:'utf8'});
    assert.notEqual(unavailable.status,0);assert.match(unavailable.stderr,/진행 중 작업이 없습니다/);
    report('scope-b','late-session','old A must not look current',repoB);
    assert.doesNotMatch(run(['render-once']).stdout,/old A must not look current/);
    run(['hook'],{session_id:'thread-b',hook_event_name:'SessionStart',cwd:repoA});
    const sessionMeta=readFileSync(join(dir,'meta.json'),'utf8');
    const sessionLive=readFileSync(join(dir,'live-turn.json'),'utf8');
    live('scope-b','turn/started',{},repoB);
    assert.equal(readFileSync(join(dir,'meta.json'),'utf8'),sessionMeta,'old attached-thread start cannot retarget the new session');
    assert.equal(readFileSync(join(dir,'live-turn.json'),'utf8'),sessionLive);
    assert.equal(trace().filter(c=>c.kind==='live').length,3);
  } finally {lateGuard.stdin.end();await new Promise(done=>lateGuard.once('exit',done));}
  if(process.env.PM_LIVE_EVIDENCE_DIR){mkdirSync(process.env.PM_LIVE_EVIDENCE_DIR,{recursive:true});writeFileSync(join(process.env.PM_LIVE_EVIDENCE_DIR,'panel-working.txt'),firstRender);writeFileSync(join(process.env.PM_LIVE_EVIDENCE_DIR,'panel-summary.txt'),rendered);writeFileSync(join(process.env.PM_LIVE_EVIDENCE_DIR,'calls.jsonl'),readFileSync(calls));}
  await until(()=>!existsSync(join(state,'runtime'))||readdirSync(join(state,'runtime')).every(p=>!p.startsWith('live-summary-')),'temporary model files cleaned');
  console.log('PASS live progress: zero automatic calls, duplicate upserts, frozen/coalesced requests, cache reuse, final cancellation/handoff, client/thread rejection, opt-out generations, late old-cwd rejection, attach-only switch invalidation, isolated mock model');
} finally {
  if(locked){guard.stdin.end();await new Promise(done=>guard.once('exit',done));}
  rmSync(gate,{force:true});
  rmSync(root,{recursive:true,force:true});
}
