// Real CLI/PTY, trusted mock native route and read-only local fixtures.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {spawn,spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';

const binary=resolve(process.argv[2]??'target/debug/maeklog');
const evidence=resolve(process.argv[3]??'target/ui-clarity-20261012/evidence/clarity');
mkdirSync(evidence,{recursive:true});
const digest=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const scriptSha=digest(new URL(import.meta.url)),candidateSha=digest(binary);
const root=mkdtempSync(join(tmpdir(),'pm-ui-clarity-')),state=join(root,'state'),repo=join(root,'maeklog');
mkdirSync(state);mkdirSync(repo);
const put=(p,v)=>writeFileSync(p,JSON.stringify(v));
const json=p=>JSON.parse(readFileSync(p,'utf8'));
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),
 HERDR_ENV:'1',HERDR_WORKSPACE_ID:'fixture',HERDR_TAB_ID:'fixture:t1',HERDR_PANE_ID:'fixture:p1',
 HERDR_SESSION:'clarity-fixture',HERDR_SOCKET_PATH:'/clarity-fixture/fake.sock',
 HERDR_BIN_PATH:resolve('scripts/mock-live-herdr.mjs'),PM_CLIENT_TOPOLOGY:join(root,'topology.json'),
 MAEKLOG_CODEX_BIN:resolve('scripts/mock-live-summary.mjs'),PM_TEST_CALLS:join(root,'model-calls.jsonl'),PM_TEST_GATE:join(root,'model-gate'),
 MAEKLOG_GH_BIN:resolve('scripts/mock-github.mjs'),
 PM_TEST_GH_CALLS:join(root,'gh-calls.jsonl'),PM_TEST_GH_FIXTURE:join(root,'github.json'),
 TERM:'xterm-256color',COLORTERM:'truecolor',TZ:'Asia/Seoul'};
delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_DISABLE_LUNA;delete env.NO_COLOR;delete env.PM_TEST_GH_DELAY_MS;delete env.PM_TEST_GH_FAIL;
const run=(args,input)=>spawnSync(binary,args,{env,input:input===undefined?undefined:JSON.stringify(input),encoding:'utf8',timeout:10000});
const ok=(args,input)=>{const r=run(args,input);assert.equal(r.status,0,JSON.stringify({args,code:r.error?.code,signal:r.signal,stderr:r.stderr}));return r.stdout.trim()?JSON.parse(r.stdout):{};};
for(const args of [['init','-q','-b','main'],['-c','user.name=Fixture','-c','user.email=fixture@invalid','commit','--allow-empty','-m','fixture']]){
 const r=spawnSync('git',args,{cwd:repo,encoding:'utf8'});assert.equal(r.status,0,r.stderr);
}
put(env.PM_CLIENT_TOPOLOGY,{socket:env.HERDR_SOCKET_PATH,panes:[{pane_id:'fixture:p1',workspace_id:'fixture',tab_id:'fixture:t1',terminal_id:'terminal-ui',pid:101,agent:'codex',cwd:repo}]});
put(env.PM_TEST_GH_FIXTURE,{});put(join(state,'client-routing.json'),{clients:[],turns:{}});
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let panel,guard,spool,sequence=0,space,journalPath;
const checks=[],captures=[],readerTraversal=[],readerNavigation=[];
const compact=s=>s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
const count=(text,token)=>text.split(token).length-1;
const isReader=()=>panel.text().includes('열람시점')&&panel.text().includes('고정된내용');
const isRecords=()=>isReader()&&panel.text().includes('전체기록·읽기전용');
const until=async(predicate,message,timeout=12000)=>{const end=Date.now()+timeout;while(Date.now()<end){if(predicate())return;await pause(30);}throw Error((typeof message==='function'?message():message)+'\n'+(panel?.text()??''));};
const live=(method,fields={})=>ok(['live','event','101','thread-ui'],{sequence:++sequence,cwd:repo,notification:{method,params:{threadId:'thread-ui',...(['turn/started','turn/completed'].includes(method)?{turn:{id:'turn-ui',status:method==='turn/started'?'inProgress':'completed',...fields}}:{turnId:'turn-ui',...fields})}}});
const capture=name=>{const path=join(evidence,name+'.ansi');writeFileSync(path,panel.frame());captures.push({name,path,rows:panel.rows,columns:panel.columns});};
const scrollTo=async token=>{for(let n=0;n<24;n++){if(panel.text().includes(token))return;panel.send('\x1b[B\x1b[B\x1b[B');await pause(120);}throw Error('main detail not reachable: '+token);};
const fileBytes=()=>Object.fromEntries([journalPath,...['task-links.json','participation.json','live-summary.json','manual-day.json','retry.json','refresh.json','rebuild.json','daily-policy.json'].map(p=>join(space,p))].map(p=>[p,existsSync(p)?readFileSync(p,'utf8'):null]));
const assertPure=before=>{for(const [p,v]of Object.entries(before))assert.equal(existsSync(p)?readFileSync(p,'utf8'):null,v,'read action changed '+p);assert(!existsSync(env.PM_TEST_GH_CALLS),'read action queried GitHub');assert(!existsSync(env.PM_TEST_CALLS),'read action invoked the configured mock model');};
const result=extra=>writeFileSync(join(evidence,'result.json'),JSON.stringify({script_sha256:scriptSha,candidate_sha256:candidateSha,checks,captures,reader_traversal:readerTraversal,reader_navigation:readerNavigation,...extra},null,2));
result({passed:false});
try{
 guard=spawn('python3',['-c','import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()',join(state,'worker.lock')],{stdio:['pipe','pipe','inherit']});
 await new Promise((done,reject)=>{guard.stdout.once('data',done);guard.once('error',reject);});
 ok(['client','attach','101','thread-ui']);
 const hook=ok(['hook'],{hook_event_name:'UserPromptSubmit',session_id:'thread-ui',turn_id:'turn-ui',cwd:repo,prompt:'그래서 적용된 거야?'});
 if(hook.hookSpecificOutput?.additionalContext)spool=JSON.parse(hook.hookSpecificOutput.additionalContext.split('\n')[0].slice('MAEKLOG_TASK_REPORT_CONTEXT_V1 '.length)).spool;
 space=readdirSync(join(state,'spaces')).map(k=>join(state,'spaces',k)).find(p=>json(join(p,'meta.json')).tab_id==='fixture:t1');
 const source=Object.values(json(join(space,'event-sources.json'))).find(s=>s.kind==='user_prompt');
 journalPath=join(state,'branches',source.context.key,'journal.json');
 // The hook normally starts a live placeholder. Prepare the separate supported
 // missing-cache state for the idle view, then recreate it via a real started event.
 const initialLive=readFileSync(join(space,'live-turn.json'),'utf8');rmSync(join(space,'live-turn.json'),{force:true});
 const at=Math.floor(Date.now()/1000),proof=[{source,quote:'격리된 화면 검증 근거'}];
 const record=(id,text,kind='completion',order=1,condition=null,outcome='reported',time=at)=>({id,key:id,kind,stage:kind==='decision'?'decision':'deployment',outcome,versions:[{operation:'add',text,condition,at_unix_utc:time,order,target:null,proof}],replaced_by:null,withdrawn:false,follows:null,relation_order:0});
 const saved={revision:1,through_order:10,through_unix_utc:at,updated_at_unix_utc:at,records:{C1:record('C1','PM 패널 9개를 교체했다는 저장된 완료 보고')},states:{P1:{key:'P1',kind:'progress',text:'PM 화면 구획 분리를 적용했다는 기록된 진행',owner:'codex',condition:null,status:'active',at_unix_utc:at,order:10,proof}},gaps:[]};
 mkdirSync(dirname(journalPath),{recursive:true});put(journalPath,saved);
 let output='',committedFrame='',committedStart=-1;const child=spawn('python3',[resolve('scripts/pty-ui-clarity.py'),binary],{env,stdio:['pipe','pipe','pipe']});
 child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
 child.stdout.on('data',s=>output+=s);child.stderr.on('data',s=>output+=s);child.stdin.on('error',()=>{});
 panel={child,rows:40,columns:80,frame:()=>{
  const start=output.lastIndexOf('\x1b[2J'),candidate=output.slice(start);
  const footerStart=candidate.lastIndexOf('\x1b['+panel.rows+';1H');
  const footerEnd=footerStart<0?-1:candidate.indexOf('\x1b[0m',footerStart);
  if(footerEnd>=0){const footer=compact(candidate.slice(footerStart,footerEnd));if(footer.includes('m메뉴')||footer.endsWith('·m')||footer.includes('Esc')){committedFrame=candidate;committedStart=start;}}
  return committedFrame;
 },text:()=>compact(panel.frame()),paint:()=>{panel.frame();return committedStart;},send:keys=>child.stdin.write(keys),output:()=>output};
 await until(()=>panel.text().includes('진행정보없음')&&panel.text().includes('PM패널9개'),'idle two-scope view missing');
 assert(!panel.text().includes('완료/새대화대기'),'queue zero became a lifecycle state');
 assert(!panel.text().includes('미요약0'),'normal zero queue still dominates main');
 assert(panel.frame().includes('\x1b[38;2;'),'normal Gruvbox ANSI missing');
 capture('idle-wide');child.kill('SIGALRM');panel.rows=42;await pause(300);capture('idle-photo-ratio');
 child.kill('SIGUSR1');panel.rows=28;panel.columns=30;await pause(300);capture('idle-narrow');
 assert(/h(?:기록)?·m(?:메뉴)?/.test(panel.text()),'narrow read/menu affordances lost');
 const initialPure=fileBytes();panel.send('\x1b[21~');await until(()=>panel.text().includes('기능선택'),'narrow menu unreachable');capture('menu-narrow');
 panel.send('\x1b');await until(()=>!panel.text().includes('기능선택')&&panel.text().includes('진행정보없음'),'narrow menu Escape did not return');assertPure(initialPure);
 child.kill('SIGUSR2');panel.rows=40;panel.columns=80;await pause(300);
 checks.push('idle/current unknown stays distinct from stored report and zero summary queue; color and photo/narrow frames');

 const description='현재 패널의 읽기 순서와 전체 기록 열람을 구현하고 검증하고 있습니다. '+
  '원문과 근거 시각을 보존하고 두 구획을 분리하여 확인합니다. '.repeat(9)+
  '실제 PM 적용은 아직 승인 전이어서 하지 않았습니다.';
 // Restore the source-bound hook snapshot; a cacheless native start has no
 // prompt receipt and must remain hidden by the production provenance guard.
 writeFileSync(join(space,'live-turn.json'),initialLive);
 live('turn/started');live('item/completed',{item:{type:'agentMessage',id:'ui-public',phase:'commentary',text:description}});
 live('turn/plan/updated',{plan:[{step:'UI 구현',status:'completed'},{step:'전체 기록과 키 동작 확인',status:'inProgress'}]});
 live('item/started',{item:{type:'commandExecution',id:'ui-command',command:'cargo test --offline',status:'inProgress'}});
 await until(()=>panel.text().includes('현재패널의읽기순서'),'current public description missing');
 const fullCurrent=compact(run(['render-once']).stdout);
 assert(fullCurrent.includes(compact(description)),'main differs from the whole collected public explanation');
 assert(fullCurrent.includes('PM패널9개')&&fullCurrent.indexOf('현재패널의읽기순서')<fullCurrent.indexOf('PM패널9개'),'stored report outranked current description');
 assert(!fullCurrent.includes('발췌'),'whole collected explanation is labelled as an excerpt');
 assert.equal(count(fullCurrent,'응답진행중'),1,'current response status must appear once with its explanation');capture('working-full');
 const pure=fileBytes();panel.send('\x1b[21~');await until(()=>panel.text().includes('기능선택'),'progress menu entry missing');
 capture('menu-safe-default');panel.send('\x1b[B\x1b[B\x1b[B\r');await until(()=>isReader()&&panel.text().includes('아직승인전이어서하지않았습니다')&&panel.text().includes('현재진행상세'),'full public explanation inaccessible');
 capture('progress-full');assertPure(pure);
 panel.send('svRr\r');await pause(300);assertPure(pure);
 panel.send('\x1b');await until(()=>!isReader()&&panel.text().includes('현재패널의읽기순서'),'progress Escape did not return');
 checks.push('current full cached explanation precedes saved reports and has one response status; progress reader pure and Escape returns');

 const records={};for(let n=1;n<=4;n++)records['D'+n]=record('D'+n,'확정 기준 '+n,'decision',10+n,n===4?'사용자가 실제 적용을 승인한 뒤에만 실행한다':null,'confirmed');
 for(let n=1;n<=6;n++)records['C'+n]=record('C'+n,'완료 보고 항목 '+n,'completion',20+n);
 records.C3.versions[0].text+=' · '+Array.from({length:80},(_,i)=>'C3 원문 구간 '+i).join(' ');
 records.F1=record('F1','과거 검증 실패가 아직 해결되지 않은 후속 문제','completion',1,null,'failed',at-3*86400);
 put(journalPath,{...saved,revision:2,records,states:{...saved.states,B1:{key:'B1',kind:'blocker',text:'실제 화면 승인 대기',owner:'user',condition:'화면 검토 뒤',status:'active',at_unix_utc:at,order:30,proof}},gaps:[{space:'old-source',generation:0,reason:'기록 수집 공백'}]});
 await until(()=>panel.text().includes('유효결정4건')&&panel.text().includes('저장상태'),'saved record update and scoped status missing');
 const defaultRender=compact(run(['render-once']).stdout);
 for(let n=1;n<=4;n++)assert(defaultRender.includes('확정기준'+n),'main lost a saved decision: '+n);
 for(let n=1;n<=6;n++)assert(defaultRender.includes('완료보고항목'+n),'main lost a saved completion: '+n);
 assert(defaultRender.includes(compact(records.C3.versions[0].text)),'main lost the tail of the long saved completion');
 assert(defaultRender.includes('실제적용을승인한뒤에만'),'decision condition lost');
 assert(defaultRender.includes('이탭의저장기록·main')&&defaultRender.includes('후속문제'),'saved scope or unresolved failure absent from main reading content');
 for(const token of ['이탭의저장기록·main','저장상태','마지막반영','수집공백','후속문제'])assert.equal(count(defaultRender,token),1,'saved scope/status duplicated or absent: '+token);
 assert(!defaultRender.includes('미반영이벤트'),'raw event count became the main pending unit');
 assert(defaultRender.includes('유효결정4건')&&defaultRender.includes('오늘완료6건'),'exact saved record totals missing');capture('many-records-warnings');
 await scrollTo('완료보고항목6');capture('completion-visible');panel.send('\x1b[H');await pause(250);
 await scrollTo('실제적용을승인한뒤에만');capture('condition-visible');
 await scrollTo('후속문제');capture('failure-visible');panel.send('\x1b[H');await pause(250);
 const savedBytes=fileBytes();panel.send('\x1b[21~\r');await until(isRecords,'F10 default is not read-only records');assertPure(savedBytes);
 panel.send('\x1b');await until(()=>!isReader()&&panel.text().includes('확정기준4'),'record return did not restore main');
 panel.send('\x1b[F');await pause(250);const anchor=panel.text();capture('main-anchor');
 panel.send('h');await until(isRecords,'h full reader missing');capture('records-full');
 const beforeReaderCopy=output.length;panel.send('\x1b[<0;1;1M\x1b[<32;4;1M');await until(()=>panel.text().includes('드래그선택중'),'reader drag copy inaccessible');
 panel.send('\x1b[<0;4;1m');await until(()=>output.slice(beforeReaderCopy).includes('\x1b]52;c;'),'reader copy request missing');
 const readerCopy=output.slice(beforeReaderCopy).match(/\x1b\]52;c;([^\x07]+)\x07/)[1];assert.equal(Buffer.from(readerCopy,'base64').toString('utf8'),'전체');capture('reader-selection');
 panel.send('\x1b');await until(()=>isRecords()&&!panel.text().includes('선택유지'),'first reader Escape did not clear only its selection');
 const beforeNarrow=panel.paint();child.kill('SIGUSR1');panel.rows=28;panel.columns=30;
 await until(()=>panel.paint()>beforeNarrow&&panel.text().includes('Esc뒤로'),'narrow reader resize missing');
 const inLongRow=()=>panel.text().includes('C3원문구간')&&!['완료보고항목6','완료보고항목5','완료보고항목4','완료보고항목2'].some(token=>panel.text().includes(token));
 // Observe each changed, completed viewport rather than queueing keys faster
 // than the panel can paint. Keep the original maximum of 84 Down events.
 for(let n=0;n<84&&!inLongRow();n++){
  const before=panel.text(),paint=panel.paint();panel.send('\x1b[B');
  await until(()=>panel.paint()>paint&&panel.text()!==before,()=>JSON.stringify({error:'reader Down did not change a completed viewport',step:n+1,frame_start:panel.paint(),visited:readerNavigation}));
  readerNavigation.push({step:n+1,frame_start:panel.paint(),text:panel.text()});
 }
 assert(inLongRow(),'could not reach the middle of a long logical reader row: '+JSON.stringify(readerNavigation));capture('reader-narrow-offset');
 const beforeWide=panel.paint();child.kill('SIGUSR2');panel.rows=40;panel.columns=80;
 await until(()=>panel.paint()>beforeWide&&panel.text().includes('Esc메인으로'),'wide reader resize missing');
 assert(panel.text().includes('C3원문구간'),'narrow-to-wide resize jumped to another record');capture('reader-wide-offset');
 const categoryPosition=()=>panel.text().match(/(\d+)\/(\d+)·/)?.slice(1).map(Number);
 const repaint=async keys=>{const before=panel.paint();panel.send(keys);await until(()=>panel.paint()>before,'reader navigation did not complete a frame');};
 await repaint('\x1b[H');
 const categoryCount=categoryPosition()?.[1];assert.equal(categoryCount,9,'record-summary processing category is missing');
 const required=['확정기준1','완료보고항목1','기록수집공백','실제적용을승인한뒤에만','미처리이벤트'],seen=new Set();
 const collect=()=>{const found=required.filter(token=>panel.text().includes(token));for(const token of found)seen.add(token);return found;};
 for(let category=1;category<=categoryCount;category++){
  await until(()=>categoryPosition()?.[0]===category,()=>JSON.stringify({error:'reader category did not paint',expected:category,actual:categoryPosition(),visited:readerTraversal}));
  await repaint('\x1b[H');const top=collect();
  await repaint('\x1b[F');
  if(category===1)await until(()=>panel.text().includes('완료보고항목1'),'reader End did not expose the oldest saved completion');
  if(category===2)await until(()=>panel.text().includes('확정기준1'),'reader End did not expose the oldest saved decision');
  readerTraversal.push({category,top,end:collect()});
  panel.send(']');await until(()=>categoryPosition()?.[0]===category%categoryCount+1,'reader next category did not paint');
 }
 const diagnostic=JSON.stringify({missing:required.filter(token=>!seen.has(token)),seen:[...seen],visited:readerTraversal});
 assert(seen.has('확정기준1')&&seen.has('완료보고항목1'),'full reader lost saved records: '+diagnostic);
 assert(seen.has('기록수집공백')&&seen.has('실제적용을승인한뒤에만'),'full reader lost exact gap reason or decision condition: '+diagnostic);
 assert(seen.has('미처리이벤트'),'raw event count lost from processing details: '+diagnostic);assertPure(savedBytes);
 panel.send('\x1b');await until(()=>panel.text()===anchor,'full reader Escape lost main scroll anchor');
 const beforeWarnings=panel.paint();child.kill('SIGUSR1');panel.rows=28;panel.columns=30;
 await until(()=>panel.paint()>beforeWarnings&&panel.frame().includes('\x1b[28;1H'),'warning view resize did not complete');
 await repaint('\x1b[H');capture('warnings-narrow');
 assert(/h(?:기록)?·m(?:메뉴)?/.test(panel.text()));
 checks.push('full saved decisions/completions and long completion tail with exact totals, conditions and one scoped failure/gap status; h all categories, reader copy/first Escape selection-only, logical-row resize and main scroll-anchor return; F10 default pure');

 const beforeSelectionResize=panel.paint();child.kill('SIGUSR2');panel.rows=40;panel.columns=80;
 await until(()=>panel.paint()>beforeSelectionResize&&panel.frame().includes('\x1b[40;1H'),'main selection resize did not complete');
 await repaint('\x1b[H');
 const before=output.length;panel.send('\x1b[<0;1;1M');
 await until(()=>panel.text().includes('드래그선택중'),'mouse down did not enter main selection');
 panel.send('\x1b[<32;5;1M');await until(()=>panel.text().includes('드래그선택중')&&panel.frame().includes('\x1b[7m'),'drag did not paint its selected cells');
 capture('selection');panel.send('\x1b[<0;5;1m');await until(()=>output.slice(before).includes('\x1b]52;c;'),'copy request missing');
 const encoded=output.slice(before).match(/\x1b\]52;c;([^\x07]+)\x07/)[1];assert(Buffer.from(encoded,'base64').toString('utf8').length>0);
 panel.send('\x1b');await until(()=>!panel.text().includes('선택유지'),'selection Escape missing');assert.equal(child.exitCode,null);assertPure(savedBytes);
 ok(['client','attach','101','thread-new']);await until(()=>!panel.text().includes('현재패널의읽기순서'),'new thread borrowed old current description');capture('new-thread');
 const switched=compact(run(['render-once']).stdout);
 assert(!switched.includes('현재패널의읽기순서'),'thread switch retained the old public explanation below the viewport');
 assert(switched.includes('완료보고항목6'),'thread switch lost independent branch records');
 checks.push('selection copy/frozen view/one-step Escape; native thread switch cannot borrow old public progress or erase branch records');
 panel.send('q');await until(()=>child.exitCode!==null,'panel cleanup failed');assert.equal(child.exitCode,0);
 assert.equal(digest(binary),candidateSha);assert.equal(digest(new URL(import.meta.url)),scriptSha);result({passed:true});
 console.log('PASS UI clarity: '+checks.join('; '));
}catch(error){if(panel)capture('failure');result({passed:false,error:String(error.stack??error)});throw error;}
finally{
 if(panel?.child.exitCode===null){panel.send('\x1b\x1bq');await pause(300);if(panel.child.exitCode===null)panel.child.kill('SIGTERM');}
 if(guard&&guard.exitCode===null){guard.stdin.end();await new Promise(done=>guard.once('exit',done));}
 if(spool)rmSync(resolve(spool,'..'),{recursive:true,force:true});rmSync(root,{recursive:true,force:true});
}
