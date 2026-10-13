// Real hooks/actions/worker processes, isolated state, mock Herdr and Luna; no API calls.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,existsSync,readdirSync,rmSync,chmodSync,unlinkSync} from 'node:fs';
import {spawn,spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(check,message,ms=8000){const end=Date.now()+ms;while(Date.now()<end){if(check())return;await pause(20);}throw new Error(message);}
const jsonLines=path=>existsSync(path)?readFileSync(path,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
for(const mode of ['incremental','latest']) {
  const root=mkdtempSync(join(tmpdir(),'pm-optout-'));
  const gate=join(root,'gates');mkdirSync(gate);
  const fixtureCwd=join(root,'fixture');mkdirSync(fixtureCwd);
  const env={...process.env,LC_ALL:'ko_KR.UTF-8',HERDR_WORKSPACE_ID:'fixture',HERDR_SESSION:'opt-out-test',
    MAEKLOG_STATE_DIR:root,MAEKLOG_PLUGIN_ROOT:process.cwd(),MAEKLOG_CODEX_BIN:resolve('scripts/mock-reducer.mjs'),
    HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),PM_TEST_ROOT:root,PM_TEST_BINARY:binary,
    PM_TEST_CALLS:join(root,'calls.jsonl'),PM_TEST_TRACE:join(root,'trace.jsonl'),PM_TEST_EVIDENCE:join(root,'evidence.jsonl'),
    PM_TEST_GATE_DIR:gate,PM_TEST_FAIL:'0'};
  delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_DISABLE_LUNA;delete env.PM_TEST_FAIL_SPACES;
  chmodSync(env.HERDR_BIN_PATH,0o755);chmodSync(env.MAEKLOG_CODEX_BIN,0o755);
  const run=(args,input)=>spawnSync(binary,args,{env,input,encoding:'utf8',timeout:5000});
  const action=name=>{const r=run(['action',name]);assert.equal(r.status,0,r.stderr);};
  const hook=(name,content='excluded',turn='turn1',session='session')=>{
    const r=run(['hook'],JSON.stringify({hook_event_name:name,session_id:session,turn_id:turn,cwd:fixtureCwd,prompt:content,last_assistant_message:content}));
    assert.equal(r.status,0,r.stderr);assert.equal(r.stderr,'');assert.equal(r.stdout.trim(),'{}');
  };
  const coordinators=[];
  const start=()=>{const child=spawn(binary,['worker'],{env,stdio:'ignore'});coordinators.push(child);return child;};
  const traces=()=>jsonLines(env.PM_TEST_TRACE);
  try {
    hook('SessionStart');
    const key=readdirSync(join(root,'spaces'))[0], space=join(root,'spaces',key);
    const put=(name,value)=>writeFileSync(join(space,name),JSON.stringify(value));
    const get=name=>JSON.parse(readFileSync(join(space,name),'utf8'));
    const raw=name=>existsSync(join(space,name))?readFileSync(join(space,name),'utf8'):null;
    const count=mode==='latest'?10:2;
    put('current.json',{revision:mode==='latest'?1:0,source_seq:0,summary:{objective:'테스트 프로젝트 목표'}});
    const events=Array.from({length:count},(_,i)=>({seq:i+1,kind:i%2?'assistant_final':'user_prompt',
      session_id:'before',content:'included before opt-out',truncated:false,recorded_at_unix:Math.floor(Date.now()/1000),cwd:fixtureCwd}));
    writeFileSync(join(space,'inbox.jsonl'),events.map(JSON.stringify).join('\n')+'\n');
    put('sequence.json',{next_seq:count+1});
    put('timeline.jsonl',{revision:0,summary:'existing timeline'});
    // A second space remains eligible when the target is off.
    const other=join(root,'spaces','other');mkdirSync(other);
    writeFileSync(join(other,'meta.json'),JSON.stringify({space_key:'other',workspace_id:'other',session_name:'test',cwd:root,label:'Other',created_at_unix:1,last_seen_at_unix:1,archived:false}));
    writeFileSync(join(other,'inbox.jsonl'),events.slice(0,2).map(e=>JSON.stringify({...e,cwd:root})).join('\n')+'\n');
    const preserved=['current.json','inbox.jsonl','timeline.jsonl','sequence.json','connection.json'];
    const before=Object.fromEntries(preserved.map(n=>[n,raw(n)]));
    writeFileSync(join(gate,'fixture'),'hold');
    const coordinator=start();
    await until(()=>traces().some(t=>t.type==='start'&&t.space==='fixture'),'model did not start');
    const model=traces().find(t=>t.type==='start'&&t.space==='fixture');
    action('disable');
    assert.equal(get('participation.json').enabled,false);
    assert.equal(get('ui.json').visible,false);
    assert.equal(get('meta.json').cwd,fixtureCwd,'toggle replaced repo cwd');
    for(const name of ['SessionStart','UserPromptSubmit','Stop','Interrupt'])hook(name,'excluded while off');
    action('disable'); // Idempotent, no generation churn.
    assert.equal(get('participation.json').generation,1);
    await until(()=>coordinator.exitCode!==null,'cancelled work did not exit');
    assert.equal(coordinator.exitCode,0);
    assert.throws(()=>process.kill(model.pid,0),{code:'ESRCH'},'model survived opt-out');
    for(const name of preserved)assert.equal(raw(name),before[name],name+' changed during opt-out');
    assert.equal(raw('latest.json'),null);
    assert.equal(get('retry.json').attempts,0,'cancellation consumed retry');
    assert.equal(JSON.parse(readFileSync(join(other,'current.json'),'utf8')).source_seq,2,'peer space stopped');
    const starts=traces().filter(t=>t.type==='start').length;
    action('startup');action('workspace-focused');
    for(const name of ['refresh','rebuild'])assert.notEqual(run(['action',name]).status,0);
    assert.equal(run(['worker']).status,0); // Separate coordinator restart obeys opt-out.
    assert.equal(traces().filter(t=>t.type==='start').length,starts);
    assert.equal(run(['panel']).status,1,'direct panel bypassed opt-out');
    // Retained rebuild request must never import the opted-out transcript on enable.
    put('rebuild.json',{session_id:'excluded-session',requested_at_unix:1});
    unlinkSync(join(gate,'fixture'));
    action('open'); // prefix+m explicitly reactivates; no full historical catch-up.
    const restartedThrough=count;
    const processed=()=>new Set(get('selected-current.json').done);
    assert.equal(get('participation.json').enabled,true);
    assert.equal(get('ui.json').visible,true);
    // A late Stop/Interrupt from an uncollected turn stays excluded even after re-enable.
    const seq=raw('sequence.json');
    hook('Stop','excluded late final','off-turn');hook('Interrupt','','off-turn');
    assert.equal(raw('sequence.json'),seq);
    await until(()=>existsSync(join(space,'selected-current.json'))&&processed().has(count),'recent-only queue did not resume');
    await until(()=>raw('inbox.jsonl').trim().split('\n').filter(Boolean).length===Math.max(0,count-6),'older input must remain manually queued');
    const afterRestart=traces().filter(t=>t.type==='start'&&t.space==='fixture');
    assert.equal(afterRestart.length,2,'restart issued multiple catch-up calls');
    assert.equal(afterRestart.at(-1).seq.length,Math.min(count,6));
    assert.equal(jsonLines(env.PM_TEST_EVIDENCE).at(-1).previous.objective,'','old brief reached restart prompt');
    action('open'); // Already enabled: opening again does not reseed or consume extra tokens.
    assert.notEqual(run(['action','rebuild']).status,0,'full transcript rebuild bypassed opt-out');
    action('hide'); // Hiding is NOT opt-out.
    assert.equal(get('participation.json').enabled,true);
    hook('UserPromptSubmit','included after re-enable','new-turn');
    hook('Stop','excluded mismatched turn','wrong-turn');
    assert.equal(get('sequence.json').next_seq,restartedThrough+2);
    hook('Stop','included final after re-enable','new-turn');
    await until(()=>processed().has(restartedThrough+2),'hidden enabled space did not collect');
    assert.equal(get('ui.json').visible,false);
    const evidence=jsonLines(env.PM_TEST_EVIDENCE);
    for(const item of evidence){
      assert.equal(item.bootstrap_history,null);
      assert(!JSON.stringify(item).includes('excluded'),'excluded conversation reached model');
    }
    assert.equal(get('participation.json').open_turns && Object.keys(get('participation.json').open_turns).length,0);
    action('clear-records');
    for(const name of ['current.json','latest.json','inbox.jsonl','timeline.jsonl','session.json','connection.json','processing.json','restart.json']) {
      assert.equal(raw(name),null,`clear-records retained ${name}`);
    }
    assert.equal(get('participation.json').enabled,false);
    action('workspace-focused');action('startup');
    assert.equal(get('participation.json').enabled,false,'focus reactivated cleared Space');
    const beforeEmptyOpen=traces().length;
    action('open'); // No transcript/queue: do not spend a model call merely to reopen.
    await pause(400);
    assert.equal(traces().length,beforeEmptyOpen);
    assert.equal(get('participation.json').enabled,true);
    hook('UserPromptSubmit','new input after clear','after-clear');
    hook('Stop','new final after clear','after-clear');
    await until(()=>existsSync(join(space,'current.json'))&&get('current.json').source_seq===2,'new dialogue after clear did not resume');
    console.log(`PASS ${mode}: in-flight cancellation, deletion-free opt-out, peer isolation, explicit open restarts from at most three collected turns in one call, no old brief/full bootstrap, idempotent open, late/mismatched Stop exclusion, hide keeps collection`);
  } finally {
    rmSync(gate,{recursive:true,force:true});
    for(const child of coordinators)if(child.exitCode===null&&child.signalCode===null)child.kill('SIGTERM');
    await pause(1000);
    rmSync(root,{recursive:true,force:true});
  }
}
