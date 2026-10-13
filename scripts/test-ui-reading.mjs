// Real CLI/PTY, source-bound native events, local fixtures and configured model/GH spies.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {spawn,spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';

const binary=resolve(process.argv[2]??'target/debug/maeklog');
const evidence=resolve(process.argv[3]??'target/ui-status-20261012/evidence/reading');
mkdirSync(evidence,{recursive:true});
const digest=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const scriptSha=digest(new URL(import.meta.url)),candidateSha=digest(binary);
const helperSha=digest(resolve('scripts/pty-ui-reading.py'));
const root=mkdtempSync(join(tmpdir(),'pm-ui-reading-')),state=join(root,'state'),repo=join(root,'demo-space');
mkdirSync(state);mkdirSync(repo);
const put=(p,v)=>writeFileSync(p,JSON.stringify(v));
const json=p=>JSON.parse(readFileSync(p,'utf8'));
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),
 HERDR_ENV:'1',HERDR_WORKSPACE_ID:'fixture',HERDR_TAB_ID:'fixture:t1',HERDR_PANE_ID:'fixture:p1',
 HERDR_SESSION:'reading-fixture',HERDR_SOCKET_PATH:'/reading-fixture/fake.sock',
 HERDR_BIN_PATH:resolve('scripts/mock-live-herdr.mjs'),PM_CLIENT_TOPOLOGY:join(root,'topology.json'),
 MAEKLOG_CODEX_BIN:resolve('scripts/mock-live-summary.mjs'),PM_TEST_CALLS:join(root,'model-calls.jsonl'),PM_TEST_GATE:join(root,'model-gate'),
 MAEKLOG_GH_BIN:resolve('scripts/mock-github.mjs'),PM_TEST_GH_CALLS:join(root,'gh-calls.jsonl'),PM_TEST_GH_FIXTURE:join(root,'github.json'),
 TERM:'xterm-256color',COLORTERM:'truecolor',TZ:'Asia/Seoul'};
for(const key of ['MAEKLOG_INTERNAL','MAEKLOG_DISABLE_LUNA','NO_COLOR','PM_TEST_GH_DELAY_MS','PM_TEST_GH_FAIL'])delete env[key];
const run=(args,input)=>spawnSync(binary,args,{env,input:input===undefined?undefined:JSON.stringify(input),encoding:'utf8',timeout:10000});
const ok=(args,input)=>{const r=run(args,input);assert.equal(r.status,0,JSON.stringify({args,code:r.error?.code,signal:r.signal,stderr:r.stderr}));return r.stdout.trim()?JSON.parse(r.stdout):{};};
const pause=ms=>new Promise(done=>setTimeout(done,ms));
const compact=s=>s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
const normalized=s=>s.replace(/\s/g,'');
const count=(s,token)=>s.split(token).length-1;
let panel,guard,spool,retrySpool,space,journalPath,sequence=0,output='',committedFrame='';
const checks=[],captures=[];
const movements={count:0,max_wait_ms:0,slow:[],stalled:[]};
const result=extra=>writeFileSync(join(evidence,'result.json'),JSON.stringify({script_sha256:scriptSha,helper_sha256:helperSha,candidate_sha256:candidateSha,checks,captures,movements,...extra},null,2));
const until=async(predicate,message,timeout=10000)=>{const end=Date.now()+timeout;while(Date.now()<end){if(predicate())return;await pause(30);}throw Error((typeof message==='function'?message():message)+'\n'+(panel?.text()??''));};
const isReader=()=>panel.text().includes('고정된내용')&&panel.text().includes('열람시점');
const brightness=ansi=>{assert(ansi.includes('\x1b[38;2;235;219;178m'),'bright Gruvbox BODY missing');assert(!ansi.includes('\x1b[38;2;168;153;132m'),'old gray foreground remains');assert(!ansi.includes('\x1b[2m'),'dim intensity remains');};
const cellWidth=text=>Array.from(text).reduce((sum,char)=>sum+(/\p{Mark}/u.test(char)?0:/[\u1100-\u115f\u2329\u232a\u2e80-\ua4cf\uac00-\ud7a3\uf900-\ufaff\ufe10-\ufe19\ufe30-\ufe6f\uff00-\uff60\uffe0-\uffe6]/u.test(char)?2:1),0);
const paintedColumns=()=>{
 const parts=panel.frame().split(/\x1b\[(\d+);(\d+)H/g);let maximum=0;
 for(let index=1;index+2<parts.length;index+=3){
  const plain=parts[index+2].replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\x1b\][^\x07]*\x07/g,'');
  maximum=Math.max(maximum,Number(parts[index+1])-1+cellWidth(plain));
 }
 return maximum;
};
const capture=(name,checkBrightness=true)=>{if(checkBrightness){brightness(panel.frame());assert(paintedColumns()<=panel.columns,'capture contains a stale wider frame');}const path=join(evidence,name+'.ansi');writeFileSync(path,panel.frame());captures.push({name,path,rows:panel.rows,columns:panel.columns,maximum_painted_columns:paintedColumns()});};
const fileBytes=()=>Object.fromEntries([journalPath,...['task-links.json','participation.json','live-summary.json','manual-day.json','retry.json','refresh.json','rebuild.json','daily-policy.json'].map(p=>join(space,p))].map(p=>[p,existsSync(p)?readFileSync(p,'utf8'):null]));
const assertPure=before=>{for(const [p,v]of Object.entries(before))assert.equal(existsSync(p)?readFileSync(p,'utf8'):null,v,'reading changed '+p);assert(!existsSync(env.PM_TEST_GH_CALLS),'reading queried GitHub');assert(!existsSync(env.PM_TEST_CALLS),'reading invoked configured mock model');};
const live=(method,fields={})=>ok(['live','event','101','thread-reading'],{sequence:++sequence,cwd:repo,notification:{method,params:{threadId:'thread-reading',...(['turn/started','turn/completed'].includes(method)?{turn:{id:'turn-reading',status:method==='turn/started'?'inProgress':'completed',...fields}}:{turnId:'turn-reading',...fields})}}});
const range=()=>{
 const footer=panel.frame().slice(panel.frame().lastIndexOf('\x1b['+panel.rows+';1H'));
 const match=compact(footer).match(/(\d+)-(\d+)\/(\d+)/);
 return match?{first:Number(match[1]),last:Number(match[2]),total:Number(match[3])}:null;
};
const home=async()=>{panel.send('\x1b[H');await until(()=>range()?.first===1&&panel.text().includes('현재대화의작업'),'main Home did not return to row 1 of the current scope');};
const resize=async(signal,columns,rows)=>{const previous=panel.frame();panel.child.kill(signal);panel.columns=columns;panel.rows=rows;await until(()=>panel.frame()!==previous&&panel.frame().includes('\x1b['+rows+';1H')&&paintedColumns()<=columns&&(columns!==77||[...panel.frame().matchAll(/─+/g)].some(match=>match[0].length===75)),'resize did not paint a complete frame at the actual requested width');};
const category=()=>panel.text().match(/(\d+)\/(\d+)·/)?.slice(1).map(Number);
const reach=async token=>{
 const limit=(range()?.total??900)+2;
 for(let n=0;n<limit;n++){
  if(panel.text().includes(normalized(token)))return;
  const prior=panel.frame(),position=range();
  if(position&&position.last===position.total)break;
  panel.send('\x1b[B');
  const started=Date.now(),end=started+10000;
  const moved=()=>position?(range()?.first??0)>position.first:panel.frame()!==prior;
  while(!moved()&&Date.now()<end)await pause(20);
  const waited=Date.now()-started;movements.count++;movements.max_wait_ms=Math.max(movements.max_wait_ms,waited);
  if(waited>=500)movements.slow.push({token,before:position,after:range(),wait_ms:waited,columns:panel.columns,rows:panel.rows});
  if(!moved()){movements.stalled.push({token,position,wait_ms:waited,columns:panel.columns,rows:panel.rows});break;}
 }
 throw Error('full reading text inaccessible: '+token+'\n'+panel.text());
};
const request='대표님은 베란다 상담 장면에서 가까이 보는 무늬와 줄눈, 끝부분을 먼저 확인하려고 합니다. '+
 '공간 경계와 바닥의 큰 인상을 보여 주는 구성도 함께 검토하고, 테이블에서는 목록을 연 채 관찰면이 남는지 다시 확인해 주세요. '+
 '전시 공간의 제품 설명을 정리한 뒤 공식 도면과 출처가 없는 항목을 구분하고, 실제 변경은 승인 뒤에만 진행해 주세요. '+
 '지금 단계에서는 실제 교체 여부를 추정하지 말아 주세요.';
const description='인계한 작업의 검수 계획 초안을 작성했습니다. 대표 보기는 공간 경계와 바닥의 큰 인상을 보여 주고, 가까이 보기는 무늬·줄눈·끝부분을 확인하도록 역할을 나누었습니다. '+
 '테이블에서는 목록을 연 채 관찰면이 남는지 다시 검수하도록 했고, 전시 200각 제품과 기존 마감의 차이를 상담 단계에서 비교하도록 정리했습니다.\n\n'+
 '공식 평면 6개와 현재 상담 장면을 대조하면서 확인된 사실과 가정인 설명을 구분했습니다. 제품명만으로 크기나 실제 배치 가능성을 확정하지 않고, 자료의 출처와 확인 시각도 각각 보존합니다. '+
 '추가 작업은 창가·바닥 중심의 관찰 위치와 동선을 먼저 확인한 뒤, 다른 공간의 조사 결과와 함께 전체 평면을 검토하고 유형·치수·배치를 결정하는 순서입니다.\n\n'+
 '상담 중 선택이 바뀌면 기존 비교 결과를 새로운 결정인 것처럼 다시 표시하지 않고, 바뀐 대상과 이전 판단의 근거를 함께 남깁니다. 아직 도면에서 확인하지 못한 치수나 제품 간격은 확인된 정보와 분리합니다. '+
 '구성 초안에 있는 장면과 실제 관찰한 장면의 차이도 각각 적어 두었습니다. 검수 보고서는 실제 반영이 완료되었다는 뜻으로 읽히지 않도록 작성했고, 사용자가 승인한 범위가 바뀌면 그 조건을 먼저 다시 확인합니다.\n\n'+
 '설명과 목록은 스크롤로 끝까지 읽을 수 있도록 검증하고 있습니다. 이번 단계에서는 실제 설치 파일을 교체하거나 수집 설정을 바꾸지 않았습니다.';
assert(description.length<1200,'fixture exceeded existing capture bound');
const title='[Studio] 베란다 대표 구성·배치 리서치 및 상담 장면 기획 — 공간 경계와 바닥의 큰 인상, 무늬·줄눈·끝부분 비교 및 테이블 관찰면 검수';
const photoExplanation='카드의 제목과 본문이 서로 다른 역할로 읽히도록 문구를 정리하고 있습니다. 생성 이미지에 대한 안내는 캡션에 한 번만 남기겠습니다.\n\n'+
 '두 번째 장에서는 제품 이름보다 색상과 표면의 특징을 먼저 설명하겠습니다. 작성한 문장과 실제 카드의 시각 구성을 함께 확인하고 있으며, 실제 게시를 완료한 것은 아닙니다.';
result({passed:false});
try{
 for(const args of [['init','-q','-b','main'],['-c','user.name=Fixture','-c','user.email=fixture@invalid','commit','--allow-empty','-m','fixture']]){
  const r=spawnSync('git',args,{cwd:repo,encoding:'utf8'});assert.equal(r.status,0,r.stderr);
 }
 put(env.PM_CLIENT_TOPOLOGY,{socket:env.HERDR_SOCKET_PATH,panes:[{pane_id:'fixture:p1',workspace_id:'fixture',tab_id:'fixture:t1',terminal_id:'terminal-reading',pid:101,agent:'codex',cwd:repo}]});
 put(env.PM_TEST_GH_FIXTURE,{});put(join(state,'client-routing.json'),{clients:[],turns:{}});
 guard=spawn('python3',['-c','import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()',join(state,'worker.lock')],{stdio:['pipe','pipe','inherit']});
 await new Promise((done,reject)=>{guard.stdout.once('data',done);guard.once('error',reject);});
 ok(['client','attach','101','thread-reading']);
 const hook=ok(['hook'],{hook_event_name:'UserPromptSubmit',session_id:'thread-reading',turn_id:'turn-reading',cwd:repo,prompt:request});
 if(hook.hookSpecificOutput?.additionalContext)spool=JSON.parse(hook.hookSpecificOutput.additionalContext.split('\n')[0].slice('MAEKLOG_TASK_REPORT_CONTEXT_V1 '.length)).spool;
 space=readdirSync(join(state,'spaces')).map(k=>join(state,'spaces',k)).find(p=>json(join(p,'meta.json')).tab_id==='fixture:t1');
 const source=Object.values(json(join(space,'event-sources.json'))).find(s=>s.kind==='user_prompt');
 journalPath=join(state,'branches',source.context.key,'journal.json');
 const initialLive=readFileSync(join(space,'live-turn.json'),'utf8');rmSync(join(space,'live-turn.json'),{force:true});
 const at=Math.floor(Date.now()/1000),proof=[{source,quote:'격리된 표시 검증 자료'}];
 const record=(id,text,kind='completion',order=1,condition=null,outcome='reported',time=at)=>({id,key:id,kind,stage:kind==='decision'?'decision':'deployment',outcome,versions:[{operation:'add',text,condition,at_unix_utc:time,order,target:null,proof}],replaced_by:null,withdrawn:false,follows:null,relation_order:0});
 const stateRow=(kind,text,order,condition=null)=>({key:kind+order,kind,text,owner:'codex',condition,status:'active',at_unix_utc:at,order,proof});
 const records={};
 for(let n=1;n<=4;n++)records['D'+n]=record('D'+n,'검증된 결정 '+n+' — 대표 보기와 가까이 보기의 역할을 나누고 자료의 출처와 관찰 위치를 각각 보존한다','decision',n,'결정 조건 '+n+' — 사용자 승인이 확인되기 전에는 실제 설치본을 교체하지 않는다','confirmed');
 const completions=[
  '공식 평면 6개를 대조하고 브라우저 화면 4종을 확인한 결과와 아직 확인되지 않은 차이를 정리했다',
  '베란다 후보 비교판과 최종 연구·출처 검증 보고서를 정리했으며 실제 전시 공간 변경은 포함하지 않았다',
  '베란다 초기 조사 보고서 PDF를 검토하고 제품 설명의 확인된 사실과 추가 확인 항목을 구분했다'
 ];
 for(let n=1;n<=3;n++)records['C'+n]=record('C'+n,'완료 보고 '+n+' — '+completions[n-1],'completion',10+n);
 records.F1=record('F1','아직 해결되지 않은 이전 검수 실패 — 상담 장면의 끝부분과 관찰면이 겹쳐 다시 확인해야 한다','completion',0,'실패 조건 — 사용자가 관찰면을 확인한 뒤에만 재검수를 통과로 기록한다','failed',at-3*86400);
 const states={};
 for(let n=1;n<=2;n++)states['P'+n]=stateRow('progress','기록된 진행 '+n+' — 후보 비교판의 제품·상담 목적표와 출처 검증 보고서를 정리하고 있다',20+n);
 for(let n=1;n<=4;n++)states['A'+n]=stateRow('action','다음 행동 '+n+' — 다른 공간의 조사 결과와 베란다 후보를 합쳐 집 전체 평면을 검토하고 유형·치수·배치를 결정한다',30+n,'행동 조건 '+n+' — 다른 공간 조사와 대표님의 승인이 모두 확인된 뒤에만 변경을 진행한다');
 states.B1=stateRow('blocker','저장 기록의 막힘 — 실제 상담 장면의 관찰면 확인이 필요하다',40,'막힘 조건 — 화면 검토가 완료되기 전에는 해결된 것으로 표시하지 않는다');
 const saved={revision:1,through_order:0,through_unix_utc:at,updated_at_unix_utc:at,records,states,gaps:[{space:'old-source',generation:0,reason:'격리 검증의 수집 공백 근거'}]};
 mkdirSync(dirname(journalPath),{recursive:true});put(journalPath,saved);
 const child=spawn('python3',[resolve('scripts/pty-ui-reading.py'),binary],{env,stdio:['pipe','pipe','pipe']});
 child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
 child.stdout.on('data',s=>output+=s);child.stderr.on('data',s=>output+=s);child.stdin.on('error',()=>{});
 panel={child,rows:44,columns:80,frame:()=>{
  const candidate=output.slice(output.lastIndexOf('\x1b[2J'));
  const start=candidate.lastIndexOf('\x1b['+panel.rows+';1H');
  const end=start<0?-1:candidate.indexOf('\x1b[0m',start);
  if(end>=0){const footer=compact(candidate.slice(start,end));if(footer.includes('m메뉴')||footer.endsWith('·m')||footer.includes('Esc'))committedFrame=candidate;}
  return committedFrame;
 },text:()=>compact(panel.frame()),send:keys=>child.stdin.write(keys)};
 await until(()=>panel.text().includes('진행정보없음'),'missing-live idle state did not render');
 assert(!panel.text().includes('완료/새대화대기'),'summary queue became lifecycle');
 const idle=compact(run(['render-once']).stdout);assert(idle.includes(normalized(request)),'request without public progress was shortened');
 await reach('지금 단계에서는 실제 교체 여부를 추정하지 말아 주세요.');brightness(panel.frame());
 checks.push('missing-live is neutral; full latest request remains reachable when public progress is absent');

 writeFileSync(join(space,'live-turn.json'),initialLive);
 live('turn/started');live('item/completed',{item:{type:'agentMessage',id:'public-reading',phase:'commentary',text:description}});
 live('item/started',{item:{type:'commandExecution',id:'check-reading',command:'cargo test --locked',status:'inProgress'}});
 const taskPath=join(space,'task-links.json'),task=json(taskPath),thread=task.threads[task.active_thread];
 thread.current=[{id:'fixture/demo-space#202',repository:'fixture/demo-space',number:202,kind:'issue',raw:'fixture/demo-space#202',url:'https://github.com/fixture/demo-space/issues/202',verification:'verified',error:null,provenance:'manual',source:{seq:source.order,uid:source.uid,thread_id:'thread-reading',turn_id:'turn-reading',generation:0,quote:'격리된 표시 검증'},next_check_at:at+3600,
  link:{kind:'issue',repository:'fixture/demo-space',number:202,url:'https://github.com/fixture/demo-space/issues/202',title,purpose:null,body:null,checked_at_unix_utc:at,error:null,state:'OPEN'}}];
 thread.next_refresh_at=at+3600;task.revision++;put(taskPath,task);
 // Photo-like unlinked dialogue: bounded history is not a collection failure.
 const verifiedCurrent=thread.current;thread.current=[];thread.candidates=verifiedCurrent;task.revision++;put(taskPath,task);
 put(journalPath,{...saved,records:{},states:{},gaps:[]});
 const currentPath=join(space,'current.json'),current=existsSync(currentPath)?json(currentPath):{};
 put(currentPath,{revision:0,base_revision:null,source_seq:0,updated_at_unix:null,...current,timing:{...current.timing,partial_history:true}});
 live('item/completed',{item:{type:'agentMessage',id:'public-reading',phase:'commentary',text:photoExplanation}});
 await home();await until(()=>panel.text().includes(normalized(photoExplanation))&&panel.text().includes('참고후보1개'),'photo-like explanation did not paint');
 await resize('SIGWINCH',77,23);
 await home();
 assert(panel.text().includes('연결정보·이슈·PR미연결·참고후보1개·l로보기'));
 assert(panel.text().includes('진행설명·')&&count(panel.text(),'응답진행중')===1,'response state must belong to the explanation heading once');
 assert(!panel.text().includes('수집공백'),'bounded history became a current collection gap');
 assert(panel.frame().includes('  카드의 제목'),'body does not have a consistent inset');
 capture('photo-unlinked');
 checks.push('photo-like unlinked header groups reference/candidate/action; inset paragraphs and one response heading; bounded history does not claim a collection gap');
 await resize('SIGUSR2',80,44);
 thread.current=verifiedCurrent;thread.candidates=[];task.revision++;put(taskPath,task);put(journalPath,saved);
 live('item/completed',{item:{type:'agentMessage',id:'public-reading',phase:'commentary',text:description}});
 await home();await until(()=>panel.text().includes('인계한작업의검수계획초안을작성했습니다'),'public progress did not render');
 const full=compact(run(['render-once']).stdout);
 assert(full.includes(normalized(description)),'main public report differs from full collected cache');
 assert(full.includes(normalized(title)),'long current work title was shortened');
 assert(!full.includes(normalized(request)),'request repeats beneath a collected public explanation');
 assert(!full.includes('공개진행설명·발췌'),'main still labels its fully rendered cache as an excerpt');
 for(let n=1;n<=4;n++)assert(full.includes('검증된결정'+n)&&full.includes('결정조건'+n),'decision or condition cap remains');
 for(let n=1;n<=3;n++)assert(full.includes('완료보고'+n),'completion cap remains');
 for(let n=1;n<=4;n++)assert(full.includes('다음행동'+n)&&full.includes('행동조건'+n),'action or condition cap remains');
 for(let n=1;n<=2;n++)assert(full.includes('기록된진행'+n),'stored progress cap remains');
 for(const token of ['수집공백','마지막반영'])assert.equal(count(full,token),1,'scoped status must appear exactly once: '+token);
 assert(!full.includes('미반영이벤트'),'raw event count became the main pending unit');
 assert(full.includes('아직해결되지않은이전검수실패'),'unresolved failure body hidden');
 assert(full.includes('실패조건'),'unresolved failure condition hidden');
 assert(full.includes('막힘조건'),'blocker condition hidden');
 thread.current[0].verification='error';thread.current[0].error='현재 참조 조회 실패 — 기존 제목은 마지막 확인 정보입니다.';task.revision++;put(taskPath,task);
 const failedReference=compact(run(['render-once']).stdout);
 assert(failedReference.includes('fixture/demo-space#202')&&failedReference.includes('현재참조조회실패'),'current reference error was hidden');
 assert(failedReference.includes(normalized(description)),'reference lookup error erased collected public progress');
 thread.current[0].verification='verified';thread.current[0].error=null;task.revision++;put(taskPath,task);
 await until(()=>panel.text().includes('fixture/demo-space#202')&&!panel.text().includes('현재참조조회실패'),'restored fixture reference did not paint');
 await resize('SIGALRM',77,44);
 capture('main-wide');await reach('이번 단계에서는 실제 설치 파일을 교체하거나 수집 설정을 바꾸지 않았습니다.');brightness(panel.frame());
 assert(panel.text().includes('m메뉴')&&panel.text().includes('h'),'scroll footer lost read/menu affordances');
 await resize('SIGUSR2',80,44);
 const pure=fileBytes();
 await home();let position=range();panel.send('\x1b[6~');await until(()=>(range()?.first??0)>position.first,'PageDown did not scroll the full main');
 position=range();panel.send(' ');await until(()=>(range()?.first??0)>position.first,'Space did not page through main content');
 position=range();panel.send('\x1b[5~');await until(()=>(range()?.first??position.first)<position.first,'PageUp did not return through main content');assertPure(pure);
 for(const token of ['검증된 결정 1','결정 조건 1','완료 보고 1','기록된 진행 1','다음 행동 1','행동 조건 1','실패 조건','막힘 조건']){
  await home();await reach(token);
 }
 await home();await reach('행동 조건 1');panel.send('\x1b[F');await until(()=>range()?.last===range()?.total,'End did not expose the last condition and final row');capture('main-actions-tail');assertPure(pure);
 const mainAnchor=panel.text();panel.send('h');await until(()=>isReader()&&panel.text().includes('전체기록·읽기전용'),'h did not open records at the scrolled main');
 panel.send('\x1b');await until(()=>!isReader()&&panel.text()===mainAnchor,'h Escape lost the full-main scroll anchor');assertPure(pure);
 checks.push('full cached public commentary and title; main all 4 decisions, 3 completions, 2 progress, 4 actions and every condition; scoped status once and unresolved failure visible');

 await resize('SIGUSR1',30,28);await home();capture('main-narrow');
 await reach('이번 단계에서는 실제 설치 파일을 교체하거나 수집 설정을 바꾸지 않았습니다.');brightness(panel.frame());
 await home();await reach('행동 조건 1');assertPure(pure);
 await resize('SIGUSR2',80,44);await home();
 panel.send('m');await until(()=>panel.text().includes('기능선택'),'m menu missing');brightness(panel.frame());
 panel.send('\r');await until(()=>isReader()&&panel.text().includes('전체기록·읽기전용'),'m default lost read-only records');
 panel.send('svRr\r');await pause(240);assertPure(pure);brightness(panel.frame());
 panel.send('\x1b');await until(()=>!isReader()&&panel.text().includes('현재대화의작업'),'records Escape did not return');
 panel.send('m');await until(()=>panel.text().includes('기능선택'),'progress menu missing');
 panel.send('\x1b[B\x1b[B\x1b[B\r');await until(()=>isReader()&&panel.text().includes('현재진행상세'),'progress reader missing');
 await reach('이번 단계에서는 실제 설치 파일을 교체하거나 수집 설정을 바꾸지 않았습니다.');
 panel.send('\x1b[H');await pause(200);capture('progress-full');
 let foundRequest=false;for(let n=0;n<(category()?.[1]??8);n++){
  if(panel.text().includes('대표님은베란다')){await reach('지금 단계에서는 실제 교체 여부를 추정하지 말아 주세요.');foundRequest=true;break;}
  const selected=category()?.[0];assert(selected,'reader category position missing');panel.send(']');
  await until(()=>category()?.[0]!==selected,'reader category navigation did not paint');
 }
 assert(foundRequest,'full latest request inaccessible in progress detail');
 const beforeCopy=output.length;panel.send('\x1b[<0;1;1M\x1b[<32;4;1M');await until(()=>panel.text().includes('드래그선택중'),'reader selection unavailable');
 panel.send('\x1b[<0;4;1m');await until(()=>output.slice(beforeCopy).includes('\x1b]52;c;'),'reader OSC52 copy request absent');
 panel.send('\x1b');await until(()=>isReader()&&!panel.text().includes('선택유지'),'first Escape closed reader instead of selection');
 panel.send('\x1b');await until(()=>!isReader()&&panel.text().includes('현재대화의작업'),'reader Escape did not return to main');assertPure(pure);
 checks.push('77/80x44 and 30x28 stay bright with full scrollable tail; record/progress details pure, full request in details, copy and two-step Escape preserved');

 const beforeCopyMain=output.length;panel.send('\x1b[<0;1;1M\x1b[<32;4;1M');await until(()=>panel.text().includes('드래그선택중'),'main selection unavailable');
 panel.send('\x1b[<0;4;1m');await until(()=>output.slice(beforeCopyMain).includes('\x1b]52;c;'),'main OSC52 copy request absent');
 panel.send('\x1b');await until(()=>!panel.text().includes('선택유지'),'main Escape did not clear selection');assertPure(pure);
 ok(['client','attach','101','thread-next']);await until(()=>!panel.text().includes('인계한작업의검수계획초안을작성했습니다'),'thread switch borrowed stale public progress');
 assert(compact(run(['render-once']).stdout).includes('완료보고1'),'thread switch erased independently scoped records');
 checks.push('main copy preserved; native thread switch cannot borrow old progress or erase branch records');
 // r acts on captured unreflected completed turns; it is not a read-only recheck.
 await home();
 const participationBytes=()=>existsSync(join(space,'participation.json'))?readFileSync(join(space,'participation.json'),'utf8'):null;
 const journalBeforeRetry=readFileSync(journalPath,'utf8'),participationBefore=participationBytes();
 panel.send('r');await until(()=>panel.text().includes('새로반영할오늘완료대화가없습니다'),'r did not explain its empty completed-turn queue');
 assert(panel.text().includes('완료응답이아직수집되지않은기록'),'r did not distinguish an unfinished captured turn');
 assert(!existsSync(env.PM_TEST_CALLS)&&!existsSync(env.PM_TEST_GH_CALLS),'empty r invoked a configured model or GitHub');
 assert.equal(readFileSync(journalPath,'utf8'),journalBeforeRetry,'empty r changed stored records');
 assert.equal(participationBytes(),participationBefore,'r changed collection participation');
 capture('retry-no-completed-turn');
 const retryHook=ok(['hook'],{hook_event_name:'UserPromptSubmit',session_id:'thread-next',turn_id:'turn-retry',cwd:repo,prompt:'오늘 완료 대화 반영을 검증합니다.'});
 if(retryHook.hookSpecificOutput?.additionalContext)retrySpool=JSON.parse(retryHook.hookSpecificOutput.additionalContext.split('\n')[0].slice('MAEKLOG_TASK_REPORT_CONTEXT_V1 '.length)).spool;
 ok(['hook'],{hook_event_name:'Stop',session_id:'thread-next',turn_id:'turn-retry',cwd:repo,last_assistant_message:'격리 검증용 작업을 완료했습니다.'});
 const queue=ok(['daily','list']);assert(queue.today_turns>0,'retry fixture has no captured completed turn');
 panel.send('r');await until(()=>panel.text().includes('기록반영을요청했습니다'),'r did not request reflection of completed captured turns');
 assert.equal(readFileSync(journalPath,'utf8'),journalBeforeRetry,'blocked fixture worker unexpectedly changed the journal');
 assert(!existsSync(env.PM_TEST_CALLS)&&!existsSync(env.PM_TEST_GH_CALLS),'guarded request escaped its isolated worker gate');
 capture('retry-requested');
 checks.push('r empty queue reports response completion wait without model/GH or history changes; r with captured today completion requests reflection through its existing worker path');
 panel.send('q');await until(()=>child.exitCode!==null,'fixture panel did not exit');assert.equal(child.exitCode,0);
 assert.equal(digest(binary),candidateSha);assert.equal(digest(new URL(import.meta.url)),scriptSha);assert.equal(digest(resolve('scripts/pty-ui-reading.py')),helperSha);
 result({passed:true,model_calls:0,github_calls:0});console.log('PASS UI reading: '+checks.join('; '));
}catch(error){if(panel)capture('failure',false);result({passed:false,error:String(error.stack??error)});throw error;}
finally{
 if(panel?.child.exitCode===null){panel.send('\x1b\x1bq');await pause(300);if(panel.child.exitCode===null)panel.child.kill('SIGTERM');}
 if(guard&&guard.exitCode===null){guard.stdin.end();await new Promise(done=>guard.once('exit',done));}
 for(const path of [spool,retrySpool])if(path)rmSync(resolve(path,'..'),{recursive:true,force:true});rmSync(root,{recursive:true,force:true});
}
