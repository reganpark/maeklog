// Daily scheduling, explicit date choice and non-mutating navigation. Mock processes only.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readdirSync,readFileSync,writeFileSync,existsSync,chmodSync,rmSync} from 'node:fs';
import {spawn,spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
import {createHash} from 'node:crypto';
const binary=resolve(process.argv[2]??'target/debug/maeklog'), artifacts=process.argv[3]?resolve(process.argv[3]):null;
const root=mkdtempSync(join(tmpdir(),'pm-daily-policy-'));
const env={...process.env,LC_ALL:'ko_KR.UTF-8',HERDR_WORKSPACE_ID:'fixture',HERDR_SESSION:'daily-test',HERDR_PANE_ID:'fixture:pm',
MAEKLOG_STATE_DIR:root,MAEKLOG_PLUGIN_ROOT:process.cwd(),MAEKLOG_CODEX_BIN:resolve('scripts/mock-reducer.mjs'),
HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),PM_TEST_ROOT:root,PM_TEST_BINARY:binary,PM_TEST_CALLS:join(root,'calls.jsonl'),PM_TEST_EVIDENCE:join(root,'evidence.jsonl'),PM_TEST_FAIL:'0',TERM:'xterm-256color'};
delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_DISABLE_LUNA;
delete env.NO_COLOR; // Capture the colored TUI, independent of the agent's shell settings.
for(const path of [env.MAEKLOG_CODEX_BIN,env.HERDR_BIN_PATH])chmodSync(path,0o755);
const run=(args,input,extra={})=>spawnSync(binary,args,{env:{...env,...extra},input,encoding:'utf8',timeout:15000});
const ok=(args,input,extra)=>{const r=run(args,input,extra);assert.equal(r.status,0,r.stderr);return r.stdout;};
const json=path=>JSON.parse(readFileSync(path,'utf8'));
const lines=path=>existsSync(path)?readFileSync(path,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
const calls=()=>lines(env.PM_TEST_CALLS);
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(check,msg){const end=Date.now()+12000;while(Date.now()<end){if(check())return;await pause(30);}throw Error(msg);}
let child,space,output='',committedFrame='',committedStart=-1,phase='setup';
const digest=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const strip=s=>s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'');
const frame=()=>{
  const start=output.lastIndexOf('\x1b[2J'),candidate=output.slice(start);
  const footerStart=candidate.lastIndexOf('\x1b[110;1H');
  const footerEnd=footerStart<0?-1:candidate.indexOf('\x1b[0m',footerStart);
  if(footerEnd>=0){committedFrame=candidate.slice(0,footerEnd+4);committedStart=start;}
  return committedFrame;
};
const capture=name=>{if(artifacts){mkdirSync(artifacts,{recursive:true});writeFileSync(join(artifacts,name+'.ansi'),frame());}};
const diagnostics=error=>{
  const names=['manual-day.json','retry.json','refresh.json','rebuild.json','live-summary.json','worker-status.json','selected-current.json'];
  return {passed:false,phase,error:String(error.stack??error),candidate_sha256:digest(binary),script_sha256:digest(new URL(import.meta.url)),
    frame_start:committedStart,panel_exit:child?.exitCode,calls:calls(),
    files:Object.fromEntries(names.map(name=>[name,space&&existsSync(join(space,name))?readFileSync(join(space,name),'utf8'):null])),
    inbox:space?lines(join(space,'inbox.jsonl')).map(event=>event.seq):[]};
};
try{
  ok(['hook'],JSON.stringify({hook_event_name:'SessionStart',session_id:'fixture-session',cwd:root}));
  space=join(root,'spaces',readdirSync(join(root,'spaces'))[0]);
  const put=(name,value)=>writeFileSync(join(space,name),JSON.stringify(value)),get=name=>json(join(space,name));
  const at=Math.floor(Date.now()/1000),old=at-3*86400;
  const event=seq=>({seq,kind:seq%2?'user_prompt':'assistant_final',session_id:'fixture-session',turn_id:'turn-'+Math.ceil(seq/2),content:'test',cwd:root,truncated:false,recorded_at_unix:seq<=4?old:seq<=16?at:0});
  const events=Array.from({length:18},(_,i)=>event(i+1));
  writeFileSync(join(space,'inbox.jsonl'),events.map(JSON.stringify).join('\n')+'\n');put('sequence.json',{next_seq:19});
  put('rebuild.json',{session_id:'must-not-import',requested_at_unix:1});
  ok(['worker']);
  assert.deepEqual(calls(),[[9,10,11,12,13,14,15,16],[5,6,7,8]]);
  assert.deepEqual(lines(join(space,'inbox.jsonl')).map(e=>e.seq),[1,2,3,4,17,18]);
  assert.equal(get('current.json').source_seq,0,'a sparse high sequence must not consume historical input');
  const queue=JSON.parse(ok(['daily','list']));
  assert.equal(queue.today_turns,0);assert.equal(Object.values(queue.manual).reduce((a,b)=>a+b,0),3);
  ok(['worker']);assert.equal(calls().length,2,'restart replayed or imported older dialogue');
  assert(lines(env.PM_TEST_EVIDENCE).every(e=>e.bootstrap_history===null&&e.previous.objective===''));
  const base=ok(['render-once']);
  assert(base.includes('진행 정보 없음'));assert(!base.includes('완료/새 대화 대기'),'summary queue was promoted to agent lifecycle');
  assert(base.replace(/\s/g,'').includes('이전·보류3턴·b날짜선택'),'held dates lost their scoped access hint');
  if(artifacts){mkdirSync(artifacts,{recursive:true});writeFileSync(join(artifacts,'daily-default.txt'),base);}
  writeFileSync(join(root,'fixture-panel'),'fixture');
  phase='date chooser';
  output='';committedFrame='';committedStart=-1;child=spawn('python3',[resolve('scripts/pty-history.py'),binary],{env,stdio:['pipe','pipe','pipe']});
  child.stdout.setEncoding('utf8');child.stdout.on('data',s=>output+=s);child.stderr.on('data',s=>output+=s.toString());
  await until(()=>frame().includes('m 메뉴'),'daily panel missing');
  if(artifacts)writeFileSync(join(artifacts,'daily-default.ansi'),frame());
  child.stdin.write('b');await until(()=>frame().includes('날짜를 선택하세요'),'date picker missing');
  if(artifacts)writeFileSync(join(artifacts,'daily-picker.ansi'),frame());
  const modalPaths=['manual-day.json','retry.json','refresh.json','rebuild.json','live-summary.json'];
  const modalBefore=Object.fromEntries(modalPaths.map(p=>[p,existsSync(join(space,p))?readFileSync(join(space,p),'utf8'):null]));
  for(const gesture of ['m','\x1b[21~','\x1b[<2;4;4M\x1b[<2;4;4m','svRr']){
    child.stdin.write(gesture);await pause(250);
    assert(frame().includes('날짜를 선택하세요'),'date picker lost input ownership');
    assert(!frame().includes('기능 선택'),'menu gesture bypassed the date picker');
    assert.equal(calls().length,2,'date navigation invoked a model');
    for(const [p,before] of Object.entries(modalBefore))assert.equal(existsSync(join(space,p))?readFileSync(join(space,p),'utf8'):null,before,'date navigation changed '+p);
  }
  const footerStart=frame().lastIndexOf('\x1b[110;1H');
  if(footerStart>=0){const footer=frame().slice(footerStart).replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'');assert(!footer.includes('m 메뉴')&&!footer.includes('r 반영'),'date picker advertised base actions');}
  await pause(300);assert.equal(calls().length,2);assert(!existsSync(join(space,'manual-day.json')));
  phase='date chooser cancel';
  child.stdin.write('\x1b');
  await until(()=>!frame().includes('날짜를 선택하세요')&&frame().includes('m 메뉴'),'date picker cancel did not return to the main view');
  assert.equal(calls().length,2);
  phase='date chooser reopen';
  child.stdin.write('b');await until(()=>frame().includes('날짜를 선택하세요'),'date picker reopen missing');
  const datedGroups=Object.keys(queue.manual).filter(day=>day!=='unknown');
  assert.equal(datedGroups.length,1,'fixture must expose exactly one dated historical group');
  phase='explicit date selection';
  child.stdin.write('\x1b[B'); // unknown is first; choose the sole dated historical group.
  await until(()=>strip(frame()).includes('▶ '+datedGroups[0]),'date picker did not paint the chosen historical date');
  capture('daily-picker-selected');
  child.stdin.write('\r');
  await until(()=>calls().length===3,'explicit date did not start a mock reduction');
  await until(()=>lines(join(space,'inbox.jsonl')).length===2,'selected historical date did not finish');
  assert.deepEqual(calls()[2],[1,2,3,4]);
  assert.equal(get('current.json').source_seq,16);
  assert.deepEqual(lines(join(space,'inbox.jsonl')).map(e=>e.seq),[17,18]);
  child.stdin.write('q');await until(()=>child.exitCode!==null,'panel did not exit');assert.equal(child.exitCode,0);
  await until(()=>spawnSync('python3',['-c','import fcntl,sys; f=open(sys.argv[1]); fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)',join(root,'worker.lock')],{stdio:'ignore'}).status===0,'manual coordinator did not release its lease');
  // Standard opt-out remains available: same headings/keys, no true-color escape sequences.
  output='';committedFrame='';committedStart=-1;child=spawn('python3',[resolve('scripts/pty-history.py'),binary],{env:{...env,NO_COLOR:'1'},stdio:['pipe','pipe','pipe']});
  child.stdout.setEncoding('utf8');child.stdout.on('data',s=>output+=s);child.stderr.on('data',s=>output+=s.toString());
  await until(()=>frame().includes('m 메뉴'),'NO_COLOR panel missing');
  assert(!frame().includes('\x1b[38;2;'));assert(frame().includes('\x1b[1m현재 대화의 작업'));
  assert.equal(calls().length,3);child.stdin.write('q');await until(()=>child.exitCode!==null,'NO_COLOR panel did not exit');assert.equal(child.exitCode,0);
  // Same work remains capped across restarts; r does not authorize unknown-date input.
  writeFileSync(join(space,'inbox.jsonl'),[event(17),event(18),{...event(19),recorded_at_unix:at},{...event(20),recorded_at_unix:at}].map(JSON.stringify).join('\n')+'\n');
  put('retry.json',{key:'daily:today:20',attempts:2,next_at:0});
  ok(['worker'],undefined,{PM_TEST_FAIL:'1'});assert.equal(calls().length,4);assert.equal(get('retry.json').attempts,3);
  const failure=ok(['render-once']);assert(failure.includes('요약 처리 실패 · 자동 재시도 중지'),'exhausted retry feedback hidden from the main scope');
  if(artifacts)writeFileSync(join(artifacts,'daily-failure.txt'),failure);
  output='';committedFrame='';committedStart=-1;child=spawn('python3',[resolve('scripts/pty-history.py'),binary],{env,stdio:['pipe','pipe','pipe']});
  child.stdout.setEncoding('utf8');child.stdout.on('data',s=>output+=s);child.stderr.on('data',s=>output+=s.toString());
  await until(()=>frame().includes('요약 처리 실패'),'actual retry failure missing in PTY');
  if(artifacts)writeFileSync(join(artifacts,'daily-failure.ansi'),frame());
  assert.equal(calls().length,4);child.stdin.write('q');await until(()=>child.exitCode!==null,'failure panel did not exit');assert.equal(child.exitCode,0);
  ok(['worker'],undefined,{PM_TEST_FAIL:'1'});assert.equal(calls().length,4);
  ok(['action','refresh']);await until(()=>get('selected-current.json').done.includes(20)&&lines(join(space,'inbox.jsonl')).map(e=>e.seq).join(',')==='17,18','explicit today retry did not finish receipt and inbox cleanup');
  assert.deepEqual(calls()[4],[19,20]);assert.deepEqual(lines(join(space,'inbox.jsonl')).map(e=>e.seq),[17,18]);
  assert.notEqual(run(['daily','request','1900-01-01']).status,0);
  ok(['action','disable']);assert.notEqual(run(['daily','request','unknown']).status,0);
  console.log('PASS daily: newest-first today only, no bootstrap/old/unknown calls, sparse receipt recovery, real b/Esc/Enter explicit date selection, m/F10/right-click modal isolation, frozen scope, retry cap and off gate');
}catch(error){
  if(artifacts){capture('failure');writeFileSync(join(artifacts,'failure-output.ansi'),output);writeFileSync(join(artifacts,'result.json'),JSON.stringify(diagnostics(error),null,2));}
  throw error;
}finally{
  if(child&&child.exitCode===null){child.stdin.write('q');await pause(500);if(child.exitCode===null)child.kill('SIGTERM');}
  rmSync(root,{recursive:true,force:true});
}
