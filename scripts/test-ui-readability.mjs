// Shipped CLI/PTY only. Anonymous fixture state, local mock providers, no user preference writes.
import assert from 'node:assert/strict';
import {mkdirSync,mkdtempSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';

const binary=resolve(process.argv[2]??'target/debug/maeklog');
const evidence=resolve(process.argv[3]??'target/ui-readability-20261013');
mkdirSync(evidence,{recursive:true});
const digest=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const hashes={candidate_sha256:digest(binary),script_sha256:digest(new URL(import.meta.url)),helper_sha256:digest(resolve('scripts/pty-ui-readability.py'))};
const captures=[],checks=[],fixtureRoots=[];
const put=(path,value)=>writeFileSync(path,JSON.stringify(value));
const json=path=>JSON.parse(readFileSync(path,'utf8'));
const pause=ms=>new Promise(done=>setTimeout(done,ms));
const plain=value=>value.replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\x1b\][^\x07]*\x07/g,'');
const compact=value=>plain(value).replace(/\s/g,'');
const count=(value,token)=>value.split(token).length-1;
const writeResult=extra=>writeFileSync(join(evidence,'result.json'),JSON.stringify({...hashes,captures,checks,fixture_roots:fixtureRoots,...extra},null,2));
const labels={
 ko:{task:'현재 대화의 작업',failed:'요약 처리 실패',stopped:'자동 재시도 중지',records:'이 탭의 저장 기록',blocker:'기록된 막힘 1건',noSource:'진행 정보 없음',source:'처리 진단 원문',processing:'기록 요약·처리 상세',menu:'기능 선택',retry:'r 오늘 재시도',held:'b 날짜 선택',completion:'오늘 완료 4건',timestamp:'상태 기록 시각',detailsHint:'h 기록의 요약·처리 상세',manual:'b 날짜 재선택'},
 en:{task:'Current conversation task',failed:'Summary processing failed',stopped:'Automatic retry stopped',records:'This tab’s saved records',blocker:'Recorded blockers: 1',noSource:'No progress information',source:'Processing diagnostic source',processing:'Record summary and processing details',menu:'Choose an action',retry:'r retry today',held:'b select date',completion:'Today’s completion reports: 4',timestamp:'Status recorded at',detailsHint:'h records: summary and processing details',manual:'b reselect date'}
};
const explanation='실행 경로를 확인했습니다. prefix + m을 Maeklog 열기에 연결하고, 기존 기록 도구는 메뉴에서 별도로 열 수 있게 준비합니다.\n\n'+
 'Maeklog는 독립된 기록 저장소로 열립니다. 이번에는 새 화면과 모델 선택을 확인하고 있으며, 기존 대화의 기록 이관은 아직 진행하지 않았습니다.';
const blocker='23개 파일에 개인 경로와 기존 플러그인 ID가 남아 있어 아직 공개할 수 없음';
const action='이름·플러그인 ID를 Maeklog 기준으로 바꾸고 남은 개인 식별 정보를 정리한다.';
const diagnostic='decision needs confirmed user evidence';
let panel,guard,activeSpool;

const cellWidth=text=>Array.from(text).reduce((sum,ch)=>sum+(/\p{Mark}/u.test(ch)?0:/[\u1100-\u115f\u2329\u232a\u2e80-\ua4cf\uac00-\ud7a3\uf900-\ufaff\ufe10-\ufe19\ufe30-\ufe6f\uff00-\uff60\uffe0-\uffe6]/u.test(ch)?2:1),0);
const paintedColumns=frame=>{
 const parts=frame.split(/\x1b\[(\d+);(\d+)H/g);let maximum=0;
 for(let i=1;i+2<parts.length;i+=3)maximum=Math.max(maximum,Number(parts[i+1])-1+cellWidth(plain(parts[i+2])));
 return maximum;
};
const until=async(predicate,message,timeout=10000)=>{
 const end=Date.now()+timeout;
 while(Date.now()<end){if(predicate())return;await pause(25);}
 throw Error(message+'\n'+(panel?.text()??''));
};
const range=()=>{
 const start=panel.frame().lastIndexOf('\x1b['+panel.rows+';1H');
 const match=compact(panel.frame().slice(start)).match(/(\d+)-(\d+)\/(\d+)/);
 return match?{first:Number(match[1]),last:Number(match[2]),total:Number(match[3])}:null;
};
const repaint=async keys=>{const before=panel.paint();panel.send(keys);await until(()=>panel.paint()>before,'key did not finish a frame');};
const home=async()=>{await repaint('\x1b[H');await until(()=>range()?.first===1,'Home did not return to row 1');};
const reach=async token=>{
 token=compact(token);
 for(let step=0;step<(range()?.total??600)+2;step++){
  if(panel.text().includes(token))return;
  const before=range();if(before?.last===before?.total)break;
  await repaint('\x1b[B');
 }
 throw Error('Full content not reachable: '+token+'\n'+panel.text());
};
const capture=(name,language)=>{
 const frame=panel.frame(),maximum=paintedColumns(frame);
 assert(maximum<=panel.columns,'stale/wide content crossed terminal bounds');
 assert(frame.includes('\x1b[38;2;235;219;178m'),'bright Gruvbox body missing');
 assert(!frame.includes('\x1b[38;2;168;153;132m')&&!frame.includes('\x1b[2m'),'gray/dim body introduced');
 const path=join(evidence,name+'.ansi');writeFileSync(path,frame);
 captures.push({name,path,language,columns:panel.columns,rows:panel.rows,maximum_painted_columns:maximum});
};
const start=async(env,width,height)=>{
 let output='',committed='',committedStart=-1;
 const child=spawn('python3',[resolve('scripts/pty-ui-readability.py'),binary,String(width),String(height)],{env,stdio:['pipe','pipe','pipe']});
 child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
 child.stdout.on('data',data=>output+=data);child.stderr.on('data',data=>output+=data);child.stdin.on('error',()=>{});
 panel={child,columns:width,rows:height,frame:()=>{
  const start=output.lastIndexOf('\x1b[2J'),candidate=output.slice(start);
  const footerStart=candidate.lastIndexOf('\x1b['+height+';1H');
  const footerEnd=footerStart<0?-1:candidate.indexOf('\x1b[0m',footerStart);
  if(footerEnd>=0){committed=candidate;committedStart=start;}
  return committed;
 },paint:()=>{panel.frame();return committedStart;},text:()=>compact(panel.frame()),send:keys=>child.stdin.write(keys),output:()=>output};
 await until(()=>panel.frame().includes('\x1b['+height+';1H'),'initial sized frame missing');
};
const stop=async()=>{
 if(!panel)return;
 panel.send('\x1b\x1bq');await until(()=>panel.child.exitCode!==null,'fixture panel did not exit');
 assert.equal(panel.child.exitCode,0);panel=null;
};
writeResult({passed:false});

try{
 for(const language of ['ko','en']){
  const root=mkdtempSync(join(evidence,'fixture-'+language+'-'));fixtureRoots.push(root);
  const state=join(root,'state'),repo=join(root,'maeklog'),config=join(root,'config');
  for(const path of [state,repo,config])mkdirSync(path);
  const env={...process.env,LC_ALL:language==='ko'?'ko_KR.UTF-8':'en_US.UTF-8',
   MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_PLUGIN_CONFIG_DIR:config,
   HERDR_ENV:'1',HERDR_WORKSPACE_ID:'fixture',HERDR_TAB_ID:'fixture:t1',HERDR_PANE_ID:'fixture:p1',
   HERDR_SESSION:'readability-fixture',HERDR_SOCKET_PATH:'/readability-fixture/fake.sock',
   HERDR_BIN_PATH:resolve('scripts/mock-live-herdr.mjs'),PM_CLIENT_TOPOLOGY:join(root,'topology.json'),
   MAEKLOG_CODEX_BIN:resolve('scripts/mock-live-summary.mjs'),PM_TEST_CALLS:join(root,'model-calls.jsonl'),PM_TEST_GATE:join(root,'model-gate'),
   MAEKLOG_GH_BIN:resolve('scripts/mock-github.mjs'),PM_TEST_GH_CALLS:join(root,'gh-calls.jsonl'),PM_TEST_GH_FIXTURE:join(root,'github.json'),
   TERM:'xterm-256color',COLORTERM:'truecolor',TZ:'Asia/Seoul'};
  for(const key of ['MAEKLOG_INTERNAL','MAEKLOG_DISABLE_LUNA','NO_COLOR','PM_TEST_GH_DELAY_MS','PM_TEST_GH_FAIL'])delete env[key];
  const run=(args,input)=>spawnSync(binary,args,{env,input:input===undefined?undefined:JSON.stringify(input),encoding:'utf8',timeout:10000});
  const ok=(args,input)=>{const result=run(args,input);assert.equal(result.status,0,result.stderr);return result.stdout.trim()?JSON.parse(result.stdout):{};};
  // Git operations are strictly within this temporary anonymous fixture.
  for(const args of [['init','-q','-b','main'],['-c','core.hooksPath=/dev/null','-c','commit.gpgsign=false','-c','user.name=Fixture','-c','user.email=fixture@invalid','commit','--allow-empty','-m','fixture']]){
   const result=spawnSync('git',args,{cwd:repo,encoding:'utf8'});assert.equal(result.status,0,result.stderr);
  }
  put(env.PM_CLIENT_TOPOLOGY,{socket:env.HERDR_SOCKET_PATH,panes:[{pane_id:'fixture:p1',workspace_id:'fixture',tab_id:'fixture:t1',terminal_id:'terminal-readability',pid:101,agent:'codex',cwd:repo}]});
  put(env.PM_TEST_GH_FIXTURE,{});put(join(state,'client-routing.json'),{clients:[],turns:{}});
  guard=spawn('python3',['-c','import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()',join(state,'worker.lock')],{stdio:['pipe','pipe','inherit']});
  await new Promise((done,reject)=>{guard.stdout.once('data',done);guard.once('error',reject);});
  ok(['client','attach','101','thread-readability']);
  const request='현재 진행과 저장된 보고를 읽기 쉬운 순서로 확인해 주세요.';
  const hook=ok(['hook'],{hook_event_name:'UserPromptSubmit',session_id:'thread-readability',turn_id:'turn-readability',cwd:repo,prompt:request});
  if(hook.hookSpecificOutput?.additionalContext)activeSpool=JSON.parse(hook.hookSpecificOutput.additionalContext.split('\n')[0].slice('MAEKLOG_TASK_REPORT_CONTEXT_V1 '.length)).spool;
  const space=readdirSync(join(state,'spaces')).map(key=>join(state,'spaces',key)).find(path=>json(join(path,'meta.json')).tab_id==='fixture:t1');
  const seed=Object.values(json(join(space,'event-sources.json'))).find(source=>source.kind==='user_prompt');
  const now=Math.floor(Date.now()/1000),midnight=Math.floor((now+32400)/86400)*86400-32400,old=midnight-360,failedAt=Math.max(midnight,now-1800),sourceMap={};
  const event=(seq,kind,content,at,session='thread-readability')=>{
   const turn=seq<4?'turn-old':seq<8?'turn-failed':'turn-readability';
   const source={...seed,uid:'fixture-source-'+seq,order:seq,kind,at_unix_utc:at,session,turn};
   sourceMap[seq]=source;
   return {seq,kind,session_id:session,turn_id:turn,model:null,content,truncated:false,recorded_at_unix:at,cwd:repo};
  };
  const baseEvents=[
   event(1,'user_prompt','기존 공개 준비 상태를 확인해 주세요.',old-3000),
   event(2,'user_prompt','작업 트리에 변경 파일 없음',old-2760),
   event(3,'assistant_final',blocker+'\n'+action,old),
   event(4,'user_prompt','결정 기록을 반영해 주세요.',failedAt),
   event(5,'user_prompt','요약 응답의 결정 기록을 준비했습니다.',failedAt),
   event(6,'user_prompt','원장 반영 전 형식을 검사합니다.',failedAt),
   event(7,'assistant_final','결정 두 건을 요약 응답으로 보고했습니다.',failedAt),
   event(8,'user_prompt',request,now)
  ];
  const other=event(9,'user_prompt','다른 브랜치의 열린 대화 기록',now,'thread-other');
  sourceMap[9].context={...seed.context,key:seed.context.key+'-other',branch:'other'};
  const journalPath=join(state,'branches',seed.context.key,'journal.json');
  const proof=seq=>[{source:sourceMap[seq],quote:baseEvents.find(event=>event.seq===seq)?.content??'익명 검증 근거'}];
  const stateRow=(key,kind,text,seq,at)=>({key,kind,text,owner:'unknown',condition:null,status:'active',at_unix_utc:at,order:seq,proof:proof(seq),uncertain_by_omission:false});
  const saved={revision:1,through_order:3,through_unix_utc:old,updated_at_unix_utc:old,records:{},states:{
   P1:stateRow('P1','progress','작업 트리에 변경 파일 없음',2,old-2760),
   B1:stateRow('B1','blocker',blocker,3,old),
   A1:stateRow('A1','action',action,3,old)},gaps:[]};
  mkdirSync(dirname(journalPath),{recursive:true});
  const taskPath=join(space,'task-links.json'),task=json(taskPath),thread=task.threads[task.active_thread];
  thread.latest_source_seq=8;thread.current_request=request;thread.next_refresh_at=now+3600;task.revision++;put(taskPath,task);
  const live=json(join(space,'live-turn.json'));Object.assign(live,{source_prompt_seq:8,revision:2,updated_at:now,ended:false,status:'inProgress',request,commentary:[{id:'public-readability',text:explanation,at_unix:now}],plan:[],activities:[],truncated:false});
  const initialCurrent={revision:0,base_revision:null,source_seq:0,updated_at_unix:null,summary:{},repo:{available:false},timing:{}};
  const writeInbox=events=>writeFileSync(join(space,'inbox.jsonl'),events.map(event=>JSON.stringify(event)).join('\n')+'\n');
  const setScenario=name=>{
   put(join(space,'current.json'),initialCurrent);rmSync(join(space,'selected-current.json'),{force:true});
   put(join(space,'cursor.json'),{last_processed_seq:0});put(join(space,'event-sources.json'),sourceMap);
   put(journalPath,saved);put(join(space,'status.json'),{state:'idle',message:'Later processing status was idle',updated_at_unix:now});
   put(join(space,'daily-retries.json'),{});
   if(name==='stale'||name==='no-source'){
    writeInbox([baseEvents[7]]);rmSync(join(space,'live-turn.json'),{force:true});
    return;
   }
   writeInbox([...baseEvents,other]);put(join(space,'live-turn.json'),live);
   const budget={key:'daily:today:7',attempts:2,next_at:now+180,validation_error:diagnostic,repeated_validation:2};
   put(join(space,'daily-retries.json'),{'daily:today:7':budget});put(join(space,'retry.json'),budget);
   if(name==='mixed'){
    const records={},successEvents=[];
    for(let number=1;number<=4;number++){
     const seq=10+(number-1)*2;
     successEvents.push(event(seq,'user_prompt','후속 성공 검증 '+number,Math.max(midnight,now-5+number),'thread-success'));
     const completion=event(seq+1,'assistant_final','후속 검증 완료 보고 '+number+' · 기존 막힘의 해결 여부는 별도로 확인한다.',Math.max(midnight,now-5+number),'thread-success');
     successEvents.push(completion);
     records['C'+number]={id:'C'+number,key:'C'+number,kind:'completion',stage:'verification',outcome:'reported',versions:[{operation:'add',text:completion.content,condition:null,at_unix_utc:completion.recorded_at_unix,order:seq+1,target:null,proof:[{source:sourceMap[seq+1],quote:completion.content}]}],replaced_by:null,withdrawn:false,follows:null,relation_order:0,corrections:[]};
    }
    writeInbox([...baseEvents,other,...successEvents]);put(join(space,'event-sources.json'),sourceMap);
    put(join(space,'selected-current.json'),{current:{...initialCurrent,revision:4,updated_at_unix:now},done:successEvents.map(event=>event.seq),display_seq:17});
    put(journalPath,{...saved,revision:5,through_order:17,through_unix_utc:now,updated_at_unix_utc:now,records});
   }
  };
  const tracked=()=>Object.fromEntries([journalPath,taskPath,...['inbox.jsonl','event-sources.json','current.json','selected-current.json','daily-retries.json','retry.json','manual-day.json','participation.json'].map(path=>join(space,path)),join(config,'language-settings.json'),join(config,'model-settings.json')].map(path=>[path,existsSync(path)?readFileSync(path,'utf8'):null]));
  const pure=before=>{for(const [path,bytes] of Object.entries(before))assert.equal(existsSync(path)?readFileSync(path,'utf8'):null,bytes,'reading changed fixture source '+path);assert(!existsSync(env.PM_TEST_CALLS)&&!existsSync(env.PM_TEST_GH_CALLS),'reading invoked a provider');};
  const allText=()=>{const result=run(['render-once']);assert.equal(result.status,0,result.stderr);return compact(result.stdout);};
  for(const width of [30,50,80]){
   const height=width===30?28:42,prefix=language+'-'+width;
   setScenario('failed');const bytes=tracked();await start(env,width,height);
   await until(()=>panel.text().includes(compact(labels[language].task)),'current section missing');
   capture(prefix+'-failed-current',language);
   const whole=allText();assert(whole.includes(compact(explanation))&&whole.includes(compact(blocker))&&whole.includes(compact(action)),'main clipped fixture body or late negation');
   assert.equal(count(whole,compact(labels[language].failed)),1,'failure status duplicated');
   assert(!whole.includes(diagnostic),'raw validator detail dominated main');
   assert(whole.includes(compact(labels[language].retry))&&whole.includes(compact(labels[language].held)),'retry/date access missing');
   await reach(labels[language].failed);await reach(labels[language].detailsHint);capture(prefix+'-failed-status',language);
   await reach('23개 파일');capture(prefix+'-recorded-blocker',language);
   const bodyRows=panel.frame().split(/\x1b\[\d+;\d+H/g).filter(row=>plain(row).includes('23개'));
   assert(bodyRows.length&&bodyRows.every(row=>row.includes('\x1b[38;2;235;219;178m')),'recorded blocker body became an active warning');
   pure(bytes);
   if(width===80){
    const legacyBudget={key:'daily:today:7',attempts:0,next_at:0,validation_error:null,repeated_validation:0};
    put(join(space,'status.json'),{state:'error',message:'Legacy raw processing failure',updated_at_unix:now});
    put(join(space,'daily-retries.json'),{});put(join(space,'retry.json'),legacyBudget);
    assert(!allText().includes(compact(labels[language].stopped)),'raw legacy error falsely proved exhausted retries');
    assert(!allText().includes(compact(labels[language].failed)),'raw legacy error bypassed recomputed daily status');
    // Observe the intermediate state so the restore cannot accept an older failed frame.
    await home();
    const pendingLabel=language==='ko'?'기록 요약 대기':'Record summary pending';
    await until(()=>panel.text().includes(compact(pendingLabel))&&!panel.text().includes(compact(labels[language].failed))&&!panel.text().includes(compact(labels[language].stopped)),'persistent legacy scenario not loaded');
    setScenario('failed');await home();
    await until(()=>panel.text().includes(compact(labels[language].failed))&&panel.text().includes(compact(labels[language].detailsHint)),'persistent failed scenario not loaded');
    await reach(labels[language].detailsHint);
    panel.send('h');await until(()=>panel.text().includes(compact('전체 기록'))||panel.text().includes('Allrecords'),'h record reader missing');
    for(let section=0;section<12&&!panel.text().includes(compact(labels[language].processing));section++)await repaint(']');
    assert(panel.text().includes(compact(labels[language].processing)),'summary-processing category missing');
    await repaint('\x1b[F');await until(()=>panel.text().includes(compact(diagnostic)),'exact diagnostic source inaccessible');
    assert(panel.text().includes(compact(labels[language].source)),'raw diagnostic source label missing');
    capture(prefix+'-processing-source',language);assert(panel.text().includes(compact(labels[language].timestamp)),'raw status timestamp label missing');pure(bytes);
    panel.send('\x1b');await until(()=>range()!==null,'record reader did not return to main');
   }
   if(width===80){
    setScenario('failed');
    const selectedDay=spawnSync('date',['-d','@'+old,'+%Y-%m-%d'],{env,encoding:'utf8'}).stdout.trim();
    put(join(space,'manual-day.json'),{day:selectedDay,seqs:[1,2,3],generation:0});
    const manualBudget={key:'daily:manual:3',attempts:2,next_at:now+180,validation_error:diagnostic,repeated_validation:2};
    put(join(space,'daily-retries.json'),{'daily:manual:3':manualBudget});put(join(space,'retry.json'),manualBudget);
    const manual=allText();assert(manual.includes(compact(labels[language].manual))&&!manual.includes(compact(labels[language].retry)),'older selected-day failure incorrectly offered r today as recovery');
    const manualBytes=tracked();await home();
    // Same-height recovery states need a visible marker, not just another key repaint.
    await until(()=>panel.text().includes(compact(labels[language].manual))&&!panel.text().includes(compact(labels[language].retry)),'persistent selected-date scenario not loaded');
    await reach(labels[language].manual);await reach(labels[language].detailsHint);capture(prefix+'-selected-date-failure',language);pure(manualBytes);
    rmSync(join(space,'manual-day.json'),{force:true});
   }
   const beforeMixedRows=range()?.total;
   setScenario('mixed');await home();await until(()=>allText().includes(compact(labels[language].completion)),'mixed completion reports not loaded');
   // render-once reads new files immediately; the persistent PTY reloads once per second.
   // Wait for its larger record set before scrolling it, then require the visible count below.
   await until(()=>range()?.total>beforeMixedRows,'persistent mixed record set not loaded');
   const mixed=allText();assert(mixed.includes(compact(labels[language].failed))&&mixed.includes(compact(blocker)),'new success incorrectly resolved old failed batch or blocker');
   for(let number=1;number<=4;number++)assert(mixed.includes(compact('후속 검증 완료 보고 '+number)),'new completion report missing');
   await reach(labels[language].completion);capture(prefix+'-mixed-success-old-failure',language);
   setScenario('stale');const staleBytes=tracked();await home();await until(()=>panel.text().includes(compact(labels[language].noSource)),'missing current source state not loaded');
   assert(!allText().includes(compact(labels[language].failed)),'idle diagnostics became a current failure');
   await reach('23개 파일');capture(prefix+'-stale-report',language);
   pure(staleBytes);assert(!allText().includes('Pendingevents')&&!allText().includes('미반영이벤트'),'raw event units leaked into main');await stop();
  }
  setScenario('no-source');await start(env,30,12);
  await until(()=>panel.text().includes(compact(labels[language].noSource)),'short missing-source state absent');
  capture(language+'-30-short-no-source',language);
  const beforeMenu=tracked();panel.send('\x1b[21~');await until(()=>panel.text().includes(compact(labels[language].menu)),'short F10 menu unreachable');
  capture(language+'-30-short-menu',language);panel.send('\x1b');await until(()=>!panel.text().includes(compact(labels[language].menu)),'short menu Escape failed');pure(beforeMenu);await stop();
  checks.push(language+': screenshot-like failed batch and stale reports at 30/50/80 columns; four newer successes retain old failure; current-source unknown and short menu; full source/diagnostic preservation; bright body and strict column bounds; zero provider calls or preference changes');
  guard.stdin.end();await new Promise(done=>guard.once('exit',done));guard=null;
  if(activeSpool){rmSync(resolve(activeSpool,'..'),{recursive:true,force:true});activeSpool=null;}
 }
 assert.equal(digest(binary),hashes.candidate_sha256);assert.equal(digest(new URL(import.meta.url)),hashes.script_sha256);
 writeResult({passed:true});console.log('PASS UI readability: '+checks.join('; '));
}catch(error){
 if(panel)capture('failure','unknown');writeResult({passed:false,error:String(error.stack??error)});throw error;
}finally{
 if(panel?.child.exitCode===null){panel.send('\x1b\x1bq');await pause(200);if(panel.child.exitCode===null)panel.child.kill('SIGTERM');}
 if(guard?.exitCode===null){guard.stdin.end();await new Promise(done=>guard.once('exit',done));}
 if(activeSpool)rmSync(resolve(activeSpool,'..'),{recursive:true,force:true});
}

