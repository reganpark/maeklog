// Real producer/report/live CLI; isolated stores, mocked Herdr, no model/network.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync,statSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {basename,join,resolve} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';

const binary=resolve(process.argv[2]??'target/debug/maeklog');
const evidence=process.argv[3]?resolve(process.argv[3]):null;
const scriptSha=createHash('sha256').update(readFileSync(new URL(import.meta.url))).digest('hex');
if(evidence){mkdirSync(evidence,{recursive:true});putRun({passed:false});}
function putRun(extra){writeFileSync(join(evidence,'run.json'),JSON.stringify({script_sha256:scriptSha,
 binary_sha256:createHash('sha256').update(readFileSync(binary)).digest('hex'),...extra},null,2));}
const root=mkdtempSync(join(tmpdir(),'pm-task-reports-'));
const state=join(root,'state'),repo=join(root,'repo'),topology=join(root,'topology.json');
mkdirSync(state);mkdirSync(repo);
const json=p=>JSON.parse(readFileSync(p,'utf8'));
const put=(p,v)=>writeFileSync(p,JSON.stringify(v));
const git=(...args)=>{
 const r=spawnSync('git',args,{cwd:repo,encoding:'utf8'});assert.equal(r.status,0,r.stderr);return r.stdout.trim();
};
git('init','-b','main');git('remote','add','origin','https://github.com/fixture/tasks.git');
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),
 HERDR_ENV:'1',HERDR_WORKSPACE_ID:'fixture',HERDR_TAB_ID:'fixture:t1',HERDR_PANE_ID:'fixture:p1',
 HERDR_SESSION:'task-test',HERDR_SOCKET_PATH:'/task-fixture/fake.sock',
 HERDR_BIN_PATH:resolve('scripts/mock-client-herdr.mjs'),PM_CLIENT_TOPOLOGY:topology,
 MAEKLOG_DISABLE_LUNA:'1',MAEKLOG_GH_BIN:resolve('scripts/mock-github.mjs'),
 PM_TEST_GH_CALLS:join(root,'gh-calls.jsonl'),PM_TEST_GH_FIXTURE:join(root,'github.json'),TERM:'xterm-256color'};
put(env.PM_TEST_GH_FIXTURE,{'list:fixture/tasks#main':[]});
delete env.MAEKLOG_INTERNAL;
put(topology,{socket:env.HERDR_SOCKET_PATH,panes:[1,2].map(n=>({pane_id:`fixture:p${n}`,
 workspace_id:'fixture',tab_id:`fixture:t${n}`,terminal_id:`terminal-${n}`,pid:100+n,agent:'codex',cwd:repo}))});
put(join(state,'client-routing.json'),{clients:[],turns:{}});
const spools=new Set(),checks=[];
const run=(args,input,extra={})=>spawnSync(binary,args,{env:{...env,...extra},input:input===undefined?undefined:JSON.stringify(input),encoding:'utf8',timeout:8000});
const ok=(args,input,extra={})=>{
 const r=run(args,input,extra);assert.equal(r.status,0,JSON.stringify({args,error:r.error?.message,signal:r.signal,stderr:r.stderr,stdout:r.stdout}));return r.stdout.trim()?JSON.parse(r.stdout):{};
};
const client=(action,n,thread)=>ok(['client',action,String(100+n),thread],undefined,{HERDR_PANE_ID:`fixture:p${n}`});
const store=n=>readdirSync(join(state,'spaces')).map(k=>join(state,'spaces',k)).find(p=>json(join(p,'meta.json')).tab_id===`fixture:t${n}`);
const task=n=>json(join(store(n),'task-links.json'));
const active=n=>{const s=task(n);return s.active_thread?s.threads[s.active_thread]:null;};
const hook=(thread,turn,prompt,event='UserPromptSubmit',extra={})=>ok(['hook'],{hook_event_name:event,session_id:thread,turn_id:turn,cwd:repo,prompt,last_assistant_message:'Fixture final',...extra},{HERDR_SOCKET_PATH:'/wrong/daemon.sock',HERDR_BIN_PATH:'/wrong/daemon/bin'});
const prompt=(thread,turn,text)=>{
 const output=hook(thread,turn,text);
 const additional=output.hookSpecificOutput?.additionalContext;assert(additional,'producer additionalContext missing');
 const relay=JSON.parse(additional.split('\n')[0].slice('MAEKLOG_TASK_REPORT_CONTEXT_V1 '.length));
 spools.add(relay.spool);assert.equal(statSync(relay.spool).mode&0o777,0o700);
 assert(!additional.includes(text));return relay;
};
const context=(relay,thread=relay.thread_id)=>ok(['task','context',relay.spool],undefined,{CODEX_THREAD_ID:thread});
const report=(relay,operation,references,quote='',revision)=>{
 const ctx=context(relay);return ok(['task','report',relay.spool],{source_uid:ctx.source_uid,
 expected_revision:revision??ctx.expected_revision,operation,references,quote},{CODEX_THREAD_ID:relay.thread_id});
};
let sequence=0;
const live=(thread,turn,n=1)=>ok(['live','event',String(100+n),thread],{sequence:++sequence,cwd:repo,
 notification:{method:'item/completed',params:{threadId:thread,turnId:turn,item:{type:'commandExecution',
 id:`fixture-${sequence}`,command:'fixture-only',status:'completed',exitCode:0}}}});
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(predicate,label,timeout=12000){
 const deadline=Date.now()+timeout;
 while(Date.now()<deadline){if(predicate())return;await pause(30);}
 throw Error(typeof label==='function'?label():label);
}
const panels=[];
function panel(n=1){
 const child=spawn('python3',[resolve('scripts/pty-history.py'),binary],{env:{...env,
  HERDR_PANE_ID:`fixture:p${n}`,HERDR_TAB_ID:`fixture:t${n}`,
  HERDR_BIN_PATH:resolve('scripts/mock-live-herdr.mjs')},stdio:['pipe','pipe','pipe']});
 let output='';child.stdout.on('data',data=>output+=data);child.stderr.on('data',data=>output+=data);child.stdin.on('error',()=>{});
 const frame=()=>output.slice(output.lastIndexOf('\x1b[2J'));
 const text=()=>frame().replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
 const p={child,frame,text,send:keys=>child.stdin.write(keys)};panels.push(p);return p;
}
async function closePanel(p){
 p.send('\x1b');await until(()=>!p.text().includes('작업연결정정'),'fixture picker failed to close');
 p.send('q');await until(()=>p.child.exitCode!==null,'fixture PM panel failed to close');assert.equal(p.child.exitCode,0,p.frame());
}
function capture(p,name){if(evidence){mkdirSync(join(evidence,'pty'),{recursive:true});writeFileSync(join(evidence,'pty',name+'.ansi'),p.frame());}}
const guard=spawn('python3',['-c','import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()',join(state,'worker.lock')],{stdio:['pipe','pipe','inherit']});
await new Promise((resolve,reject)=>{guard.stdout.once('data',resolve);guard.once('error',reject);});
try {
 client('attach',1,'thread-a');client('attach',2,'thread-b');
 // Native attachment exists from the old helper, but the new task state has
 // never existed. No user prompt is supplied to hide initialization defects.
 rmSync(join(store(1),'task-links.json'));
 let p=panel();
 await until(()=>existsSync(join(store(1),'task-links.json'))&&task(1).active_thread==='thread-a'&&p.text().includes('진행정보없음')&&p.text().includes('현재대화의작업'),()=>JSON.stringify({error:'registered running client did not warm-initialize',frame:p.text()}));
 assert.deepEqual(active(1).current,[]);assert.equal(active(1).current_request,'');capture(p,'warm-init');
 await closePanel(p);
 client('detach',1,'thread-a');p=panel();
 await until(()=>p.text().includes('진행정보없음')&&p.text().includes('현재대화의작업'),'unbound main card missing');
 p.send('l');await until(()=>p.text().includes('작업연결정정')&&p.text().includes('지금은참조보기만'),()=>JSON.stringify({error:'no-client picker did not explain read-only boundary',frame:p.text()}));
 const beforeUnbound=task(1).revision;p.send('a\r');await pause(300);assert.equal(task(1).revision,beforeUnbound);
 assert.equal(task(1).active_thread,null);capture(p,'unbound-picker');
 p.child.kill('SIGALRM');await until(()=>p.text().includes('지금은참조보기만'),'narrow unbound picker missing');await pause(120);capture(p,'unbound-picker-narrow');
 await closePanel(p);client('attach',1,'thread-a');
 checks.push('registered old client initializes task state before any new prompt; genuine no-client picker stays open-only');
 const a=prompt('thread-a','a1','fixture/tasks#142 작업을 진행해 주세요.');
 const b=prompt('thread-b','b1','other/tasks#142 작업을 진행해 주세요.');
 assert.equal(active(1).current[0].id,'fixture/tasks#142');assert.equal(active(2).current[0].id,'other/tasks#142');
 assert.equal(context(a).thread_id,'thread-a');assert.equal(context(b).thread_id,'thread-b');
 const initial=context(a);assert(initial.current.every(x=>typeof x==='string'));
 assert(!('request' in initial)&&!('link' in initial));
 assert.notEqual(run(['task','context',a.spool],undefined,{CODEX_THREAD_ID:'thread-b'}).status,0);
 const childHook=hook('thread-a','child1','fixture/tasks#999 작업을 진행해 주세요.','UserPromptSubmit',{agent_id:'child',agent_type:'worker'});
 assert.deepEqual(childHook,{});assert.equal(active(1).current[0].id,'fixture/tasks#142');
 checks.push('same-number cross-repo/tab isolation, daemon environment ignored, compact projection, child hook/runtime rejected');

 const queued=report(a,'related',['https://github.com/fixture/tasks/pull/285']);assert.equal(queued.status,'queued');
 live('thread-a','a1');
 const after=context(a);assert.equal(after.pending,false);assert.equal(after.receipts[0].status,'processed');
 assert(after.expected_revision>initial.expected_revision);
 const second=report(a,'reference',['other/tasks#143']);assert.equal(second.status,'queued');live('thread-a','a1');
 assert.equal(context(a).receipts.length,2);assert.equal(active(2).current[0].id,'other/tasks#142');
 checks.push('live completion drains sequential reports with latest receipt/revision and no PM panel');

 // A fresh revision cannot turn a same-source late report into a new user choice.
 const manual=task(1),ma=manual.threads[manual.active_thread];
 ma.manual_barrier_seq=ma.latest_source_seq;ma.explicit_none=true;ma.current=[];manual.revision++;
 put(join(store(1),'task-links.json'),manual);
 // Simulate the UI transaction above; the transport/backend must still reject old intent.
 report(a,'replace',['fixture/tasks#142'],'fixture/tasks#142 작업을 진행해 주세요.',manual.revision);
 live('thread-a','a1');assert.equal(active(1).explicit_none,true);assert.deepEqual(active(1).current,[]);
 assert.equal(context(a).receipts.at(-1)?.status,'processed');
 checks.push('manual source barrier survives receipt revision reread without restoring the old target');

 report(a,'reference',['fixture/tasks#144']);
 const c=prompt('thread-a','a2','fixture/tasks#143 작업을 진행해 주세요.');
 assert.equal(active(1).current[0].id,'fixture/tasks#143');
 assert(existsSync(join(a.spool,'retired.json')));
 assert(readdirSync(a.spool).filter(x=>x.startsWith('receipt-')).some(x=>json(join(a.spool,x)).status==='rejected'));
 assert.notEqual(run(['task','context',a.spool],undefined,{CODEX_THREAD_ID:'thread-a'}).status,0);
 checks.push('explicit switch changes displayed intent immediately and terminates stale queued source with rejected receipt');

 // The Stop host path is a short-timeout regression for nested participation locks.
 report(c,'reference',['fixture/tasks#146']);
 const started=Date.now();hook('thread-a','a2','', 'Stop');assert(Date.now()-started<8000);
 assert.equal(context(c).pending,false);assert(context(c).receipts.every(x=>x.status==='processed'));
 report(c,'reference',['fixture/tasks#147']);live('thread-a','a2');
 assert(existsSync(join(c.spool,'retired.json')));
 assert.equal(active(1).current[0].id,'fixture/tasks#143');
 checks.push('Stop queue drain has no nested-lock deadlock; ended-turn reports retire instead of changing current');

 client('attach',1,'thread-new');assert.equal(active(1).thread_id,'thread-new');assert.deepEqual(active(1).current,[]);
 hook('thread-a','a2','', 'Stop');assert.equal(task(1).active_thread,'thread-new');
 client('attach',1,'thread-a');assert.equal(active(1).current[0].id,'fixture/tasks#143');
 client('detach',1,'thread-a');assert.equal(task(1).active_thread,null);
 // Proven old turn routes can still receive late hooks, but cannot reactivate.
 hook('thread-a','a2','fixture/tasks#999 작업을 진행해 주세요.');assert.equal(task(1).active_thread,null);
 checks.push('attach/new hides old card before prompt, resume restores own references, late Stop/prompt cannot reactivate detached thread');

 client('attach',1,'thread-a');
 const d=prompt('thread-a','a3','fixture/tasks#148 작업을 진행해 주세요.');
 rmSync(d.spool,{recursive:true});live('thread-a','a3');
 assert(!existsSync(join(store(1),'task-report-host.json')));
 const regenerated=prompt('thread-a','a3','fixture/tasks#148 작업을 진행해 주세요.');
 assert.notEqual(regenerated.spool,d.spool);assert(existsSync(regenerated.spool));
 checks.push('lost /tmp queue cleans private host record and same-source prompt regenerates a valid spool');

 // Four outstanding messages bound sender writes; duplicate is idempotent.
 const ctx=context(regenerated);
 const payload=n=>({source_uid:ctx.source_uid,expected_revision:ctx.expected_revision,operation:'reference',references:[`fixture/tasks#${n}`],quote:''});
 const putReport=p=>run(['task','report',regenerated.spool],p,{CODEX_THREAD_ID:'thread-a'});
 for(let n=200;n<204;n++)assert.equal(putReport(payload(n)).status,0);
 assert.equal(putReport(payload(200)).status,0);assert.notEqual(putReport(payload(204)).status,0);
 assert.notEqual(putReport({...payload(200),quote:'x'.repeat(17000)}).status,0);
 const expired=readdirSync(regenerated.spool).find(x=>x.startsWith('request-'));
 const value=json(join(regenerated.spool,expired));value.sent_at=Math.floor(Date.now()/1000)-601;put(join(regenerated.spool,expired),value);
 live('thread-a','a3');
 const receipts=context(regenerated).receipts;assert.equal(receipts.length,4);
 assert.equal(receipts.filter(x=>x.status==='processed').length,1);
 assert.equal(receipts.filter(x=>x.status==='rejected').length,3);
 assert.equal(receipts.filter(x=>x.error?.includes('만료')).length,1);
 assert.equal(receipts.filter(x=>x.error?.includes('현재 대화 또는 작업이 변경')).length,2);
 checks.push('16KiB/four pending bounds, duplicate idempotency, expired-message rejection ack, no retry loop');
 const gh=existsSync(env.PM_TEST_GH_CALLS)?readFileSync(env.PM_TEST_GH_CALLS,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
 assert(gh.every(args=>args[0]==='pr'&&args[1]==='list'),'transport validation unexpectedly queried GitHub reference details');
 assert(!existsSync(join(root,'model-calls.jsonl')));

 // The visual/metadata fixture deliberately keeps observed main records while
 // the primary agent reports a separate, verifiable feature worktree.
 git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','--allow-empty','-m','initial');
 const worktree=join(root,'feature-worktree');git('worktree','add','-b','feature/dialogue-work',worktree);
 const e=prompt('thread-a','a4','fixture/tasks#142 작업을 진행해 주세요.');
 const ec=context(e);
 ok(['task','report',e.spool],{source_uid:ec.source_uid,expected_revision:ec.expected_revision,
  operation:'related',references:['https://github.com/fixture/tasks/pull/285'],quote:'',worktree,created:true},
  {CODEX_THREAD_ID:'thread-a'});live('thread-a','a4');
 const sources=json(join(store(1),'event-sources.json'));
 const source=sources[active(1).latest_source_seq],journalPath=join(state,'branches',source.context.key,'journal.json');
 put(journalPath,{revision:1,through_order:1,records:{'D-main-fixture':{id:'D-main-fixture',key:'main-policy',
  kind:'decision',stage:'decision',outcome:'confirmed',versions:[{operation:'add',text:'main의 기존 배포 정책을 유지한다',
   condition:null,at_unix_utc:Math.floor(Date.now()/1000),order:1,target:null,proof:[{source,quote:'기존 배포 정책 유지'}]}],
  replaced_by:null,withdrawn:false,follows:null,relation_order:0}}});
 const journalBefore=readFileSync(journalPath,'utf8');
 const issue=n=>({number:n,html_url:`https://github.com/fixture/tasks/issues/${n}`,title:`작업 이슈 ${n}`,
  body:'## 목적\n대화의 작업 참조를 확인한다.',state:'open'});
 const fixtures={'list:fixture/tasks#main':[],
  'api:fixture/tasks#142':issue(142),'api:fixture/tasks#143':{__exit:1,__stderr:'fixture authorization unavailable'},
  'api:fixture/tasks#145':issue(145),'api:fixture/tasks#146':issue(146),
  'api:fixture/tasks#285':{number:285,html_url:'https://github.com/fixture/tasks/pull/285',pull_request:{html_url:'https://github.com/fixture/tasks/pull/285'}},
  'pr:fixture/tasks#285':{number:285,url:'https://github.com/fixture/tasks/pull/285',title:'대화 기반 연결 표시 개선',
   body:'## 변경 범위\n현재 작업 참조와 저장된 브랜치 기록을 구분한다.',state:'OPEN',headRefName:'feature/dialogue-work',
   headRefOid:git('rev-parse','HEAD'),headRepository:{name:'tasks',nameWithOwner:'fixture/tasks'},headRepositoryOwner:{login:'fixture'},
   isCrossRepository:false,closingIssuesReferences:[{number:142,url:'https://github.com/fixture/tasks/issues/142'}]}};
 put(env.PM_TEST_GH_FIXTURE,fixtures);p=panel();
 await until(()=>active(1).current[0].verification==='verified'&&active(1).related_pr.some(r=>r.number===285)&&
  p.text().includes('이탭의저장기록·main')&&p.text().includes('main의기존배포정책')&&
  p.text().includes('tasks#285')&&p.text().includes('인계한작업위치')&&p.text().includes('feature/dialogue-work'),()=>JSON.stringify({error:'current/related/worktree/main record separation missing',frame:p.text(),task:active(1)}));
 assert.equal(active(1).worktree.verification,'verified');capture(p,'current-related-main');
 p.send('l');await until(()=>p.text().includes('작업연결정정')&&p.text().includes('관련참조'),'related picker missing');capture(p,'current-related-picker');
 p.send('\x1b');await until(()=>!p.text().includes('작업연결정정'),'related picker did not close');
 prompt('thread-a','a5','fixture/tasks#142 말고 fixture/tasks#143 작업을 진행해 주세요.');
 await until(()=>active(1).current?.[0]?.number===143&&active(1).current[0].verification==='error'&&
  p.text().includes('확인실패')&&p.text().includes('이전연결'),()=>JSON.stringify({error:'B lookup failure restored or hid intent',frame:p.text(),task:active(1)}));
 assert.equal(active(1).previous.at(-1).current[0].number,142);capture(p,'replacement-error-previous');
 p.send('l');await until(()=>p.text().includes('작업연결정정')&&p.text().includes('이전연결'),'previous picker missing');capture(p,'replacement-picker');
 p.send('\x1b');await until(()=>!p.text().includes('작업연결정정'),'previous picker did not close');
 const multi=prompt('thread-a','a6','fixture/tasks#145와 fixture/tasks#146 작업을 진행해 주세요.');
 await until(()=>active(1).current.length===2&&active(1).current.every(r=>r.verification==='verified')&&
  p.text().includes('tasks#145')&&p.text().includes('tasks#146'),'multiple explicit targets were collapsed');
 capture(p,'multiple-targets');p.child.kill('SIGALRM');await pause(250);capture(p,'multiple-targets-narrow');
 p.send('o');await until(()=>p.text().includes('작업연결정정')&&p.text().includes('연결이여러개'),'multi-target o skipped selection');capture(p,'multiple-picker-narrow');
 p.send('a');await until(()=>p.text().includes('번호만입력해도'),'narrow manual form missing');capture(p,'manual-input-narrow');
 p.send('\x1b');await until(()=>!p.text().includes('번호만입력해도'),'narrow input cancellation missing');
 await closePanel(p);assert.equal(readFileSync(journalPath,'utf8'),journalBefore,'task association reassigned or rewrote main journal');
 // The PTY fixture shares the source's environment, not its real pane. Reflect
 // its completed unregister before exercising destructive fixture-only clear;
 // the live mocked Codex pane remains protected and present in topology.
 const uiPath=join(store(1),'ui.json'),closedUI=json(uiPath);
 assert.equal(closedUI.visible,false);assert.notEqual(p.child.exitCode,null);
 put(uiPath,{...closedUI,panel_pane_id:null});
 checks.push('read-only metadata verifies issue kind/related PR/worktree; B error preserves explicit intent+A previous; multiple targets and narrow chooser; main records unchanged');
 const finalTask=task(1),peerState=readFileSync(join(store(2),'task-links.json'),'utf8'),peerContext=readFileSync(join(b.spool,'context.json'),'utf8');
 report(multi,'reference',['fixture/tasks#777'],'fixture/tasks#145와 fixture/tasks#146 작업을 진행해 주세요.');
 const cleared=run(['action','clear-records']);assert.equal(cleared.status,0,cleared.stderr);
 assert(!existsSync(join(store(1),'task-links.json')));assert(!existsSync(join(store(1),'task-report-host.json')));
 for(const spool of spools)if(spool!==b.spool)assert(!existsSync(spool),'explicit clear retained own current/retired temporary data '+spool);
 assert.equal(readFileSync(join(b.spool,'context.json'),'utf8'),peerContext);
 assert.equal(readFileSync(join(store(2),'task-links.json'),'utf8'),peerState);
 const clearedJournal=json(journalPath),beforeClearJournal=JSON.parse(journalBefore);
 assert.deepEqual(clearedJournal.records,beforeClearJournal.records,'explicit clear changed shared main records or their proof');
 assert.equal(clearedJournal.through_order,beforeClearJournal.through_order);
 assert.deepEqual(clearedJournal.gaps,[{space:basename(store(1)),generation:json(join(store(1),'participation.json')).generation,
  reason:'Space 수집 중지 · 마지막 확인 기록'}],'explicit clear added an unrelated gap or omitted its own collection-stop gap');
 checks.push('explicit private deletion removes current and retired report quotes while peer task/spool and shared main records+proof survive with a collection-stop gap');
 if(evidence){mkdirSync(evidence,{recursive:true});writeFileSync(join(evidence,'task-report-result.json'),JSON.stringify({checks,final_before_explicit_clear:finalTask},null,2));}
 assert.equal(createHash('sha256').update(readFileSync(new URL(import.meta.url))).digest('hex'),scriptSha,'fixture changed while running');
 if(evidence)putRun({passed:true,checks});
 console.log('PASS task report relay: '+checks.join('; '));
}catch(error){
 if(evidence){putRun({passed:false,error:String(error.stack??error)});for(const [i,p]of panels.entries())capture(p,'failure-panel-'+i);}
 throw error;
}finally{
 for(const p of panels)if(p.child.exitCode===null){p.send('\x1bq');await pause(200);if(p.child.exitCode===null)p.child.kill('SIGTERM');}
 guard.stdin.end();await new Promise(resolve=>guard.once('exit',resolve));
 for(const spool of spools)rmSync(spool,{recursive:true,force:true});
 rmSync(root,{recursive:true,force:true});
}
