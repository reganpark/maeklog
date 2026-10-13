// Real PM CLI/panel, isolated Git repositories, mock GitHub/Herdr/opener.
// No network, model request, GitHub mutation or user-record write is possible.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,readdirSync,existsSync,rmSync,chmodSync} from 'node:fs';
import {spawnSync,spawn} from 'node:child_process';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
import {createHash} from 'node:crypto';

const binary=resolve(process.argv[2]??'target/debug/maeklog');
const artifacts=process.argv[3]?resolve(process.argv[3]):null;
const scriptSha=createHash('sha256').update(readFileSync(new URL(import.meta.url))).digest('hex');
function putRun(extra){writeFileSync(join(artifacts,'run.json'),JSON.stringify({script_sha256:scriptSha,
  binary_sha256:createHash('sha256').update(readFileSync(binary)).digest('hex'),...extra},null,2));}
if(artifacts){mkdirSync(artifacts,{recursive:true});putRun({passed:false});}
const cases=[];
const repository='fixture/work-links';
const branchName='feature/work-links';
const id=(kind,number,repo=repository)=>`${kind==='issue'?'Issue':'Pr'}:${repo}#${number}`;
const now=()=>Math.floor(Date.now()/1000);
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const json=path=>JSON.parse(readFileSync(path,'utf8'));
const lines=path=>existsSync(path)?readFileSync(path,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
async function until(check,message,timeout=12000){
  const end=Date.now()+timeout;
  while(Date.now()<end){if(check())return;await pause(30);}
  throw Error(typeof message==='function'?message():message);
}
function fixtureCase(name,{remote=true}={}){
  const root=mkdtempSync(join(tmpdir(),'pm-links-'+name+'-'));
  const repo=join(root,'repo'),state=join(root,'state');
  mkdirSync(repo);
  const git=(...args)=>{
    const result=spawnSync('git',args,{cwd:repo,encoding:'utf8'});
    assert.equal(result.status,0,result.stderr);return result.stdout.trim();
  };
  git('init','-b',branchName);
  git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','--allow-empty','-m','initial');
  if(remote)git('remote','add','origin','https://github.com/'+repository+'.git');
  const head=git('rev-parse','HEAD');
  const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),
    HERDR_WORKSPACE_ID:'fixture',HERDR_PANE_ID:'fixture:pm',HERDR_SESSION:'links-test',
    HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),PM_TEST_ROOT:root,PM_TEST_BINARY:binary,
    MAEKLOG_GH_BIN:resolve('scripts/mock-github.mjs'),PM_TEST_GH_CALLS:join(root,'gh-calls.jsonl'),
    PM_TEST_GH_FIXTURE:join(root,'github.json'),MAEKLOG_DISABLE_LUNA:'1',TZ:'Asia/Seoul',TERM:'xterm-256color'};
  delete env.MAEKLOG_INTERNAL;delete env.PM_TEST_GH_FAIL;delete env.PM_TEST_GH_DELAY_MS;
  for(const path of [env.HERDR_BIN_PATH,env.MAEKLOG_GH_BIN])chmodSync(path,0o755);
  const run=(args,extra={},input)=>spawnSync(binary,args,{env:{...env,...extra},input,encoding:'utf8',timeout:25000});
  const ok=(args,extra={},input)=>{
    const result=run(args,extra,input);
    assert.equal(result.status,0,JSON.stringify({args,error:result.error?{code:result.error.code,message:result.error.message}:null,signal:result.signal,stderr:result.stderr?.slice(-2000),stdout:result.stdout?.slice(-2000)}));
    return result.stdout;
  };
  ok(['hook'],{},JSON.stringify({hook_event_name:'SessionStart',session_id:'fixture-session',cwd:repo}));
  const space=join(state,'spaces',readdirSync(join(state,'spaces'))[0]);
  const fixture={};
  const save=()=>writeFileSync(env.PM_TEST_GH_FIXTURE,JSON.stringify(fixture));
  const list=(rows,branch=branchName,base=repository)=>{fixture['list:'+base+'#'+branch]=rows;save();};
  const get=()=>JSON.parse(ok(['links','show']));
  const refresh=(extra={})=>{ok(['links','refresh'],extra);return get();};
  const calls=()=>lines(env.PM_TEST_GH_CALLS);
  const directory=(branch=git('branch','--show-current'))=>{
    const roots=join(state,'branches');
    return readdirSync(roots).map(key=>join(roots,key)).find(path=>{
      const context=join(path,'context.json');
      return existsSync(context)&&json(context).branch===branch;
    });
  };
  const cache=()=>join(directory(),'links.json');
  const value={name,root,repo,state,space,head,git,env,run,ok,fixture,save,list,get,refresh,calls,directory,cache,child:null,jobs:[]};
  cases.push(value);return value;
}
function issue(number,repo=repository){
  return {number,url:`https://github.com/${repo}/issues/${number}`,title:`이슈 ${number}`,body:`## 목적\n이슈 ${number}의 목적을 확인한다.`,state:'OPEN'};
}
function pr(number,{head,branch=branchName,repo=repository,source=repo,state='OPEN',issues=[142]}={}){
  const [owner,name]=source.split('/');
  return {number,url:`https://github.com/${repo}/pull/${number}`,title:`PR ${number}`,body:`## 변경 범위\nPR ${number}의 범위를 확인한다.`,
    state,headRefName:branch,headRefOid:head,headRepository:{name,nameWithOwner:source},
    headRepositoryOwner:{login:owner},isCrossRepository:source!==repo,
    closingIssuesReferences:issues.map(number=>issue(number,repo))};
}
function responses(c,rows){
  for(const row of rows){
    c.fixture['pr:'+repository+'#'+row.number]=row;
    for(const reference of row.closingIssuesReferences??[])c.fixture['issue:'+repository+'#'+reference.number]=reference;
  }
  c.list(rows);
}
function assertReadOnly(c){
  assert(c.calls().every(args=>(['issue','pr'].includes(args[0])&&['list','view'].includes(args[1])&&!args.includes('--web'))||
    (args.length===4&&args[0]==='api'&&args[1]==='--method'&&args[2]==='GET'&&/^repos\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/issues\/[1-9][0-9]*$/.test(args[3]))));
  assert(!existsSync(join(c.root,'model-calls.jsonl')));
}
function panel(c){
  writeFileSync(join(c.root,'fixture-panel'),'fixture');
  let output='';
  const child=spawn('python3',[resolve('scripts/pty-history.py'),binary],{env:c.env,stdio:['pipe','pipe','pipe']});
  c.child=child;
  child.stdin.on('error',()=>{}); // The PTY can exit between close/cancel keystrokes.
  child.stdout.on('data',data=>output+=data.toString());child.stderr.on('data',data=>output+=data.toString());
  child.on('error',error=>output+=error.message);
  const frame=()=>output.slice(output.lastIndexOf('\x1b[2J'));
  const text=()=>frame().replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
  const send=async keys=>{child.stdin.write(keys);await pause(150);};
  const p={child,frame,text,send};c.panel=p;return p;
}
async function closePanel(p){
  for(const keys of ['\x1b','\x1b','q'])if(p.child.exitCode===null)await p.send(keys);
  await until(()=>p.child.exitCode!==null,'fixture panel did not close');
  assert.equal(p.child.exitCode,0,p.frame().slice(-2000));
}
function capture(p,name){
  if(!artifacts)return;
  mkdirSync(artifacts,{recursive:true});
  writeFileSync(join(artifacts,name+'.ansi'),p.frame());
  writeFileSync(join(artifacts,name+'.txt'),p.frame().replace(/\x1b\[[0-9;?]*[A-Za-z]/g,''));
}

try{
  // An unpushed local commit must not disconnect the branch's unique open PR.
  const open=fixtureCase('open');
  open.git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','--allow-empty','-m','unpushed change');
  assert.notEqual(open.git('rev-parse','HEAD'),open.head);
  const current=pr(167,{head:open.head});
  responses(open,[pr(180,{head:open.head,state:'MERGED',issues:[180]}),pr(170,{head:open.head,source:'fork/work-links',issues:[170]}),current]);
  let links=open.refresh();
  assert.equal(links.primary_pr,id('pr',167),'unique source-repository PR lost after local commit');
  assert.equal(links.primary_issue,id('issue',142),'issue must come from the selected PR');
  assert.equal(links.pr_state,'linked');assert.equal(links.issue_state,'linked');
  assert.equal(links.entries[id('pr',167)].head_oid,open.head);
  assert(!links.issue_candidates.includes(id('issue',170))&&!links.issue_candidates.includes(id('issue',180)),'unselected PRs supplied automatic issue candidates');
  const count=open.calls().length;
  open.ok(['render-once']);open.ok(['render-history']);open.ok(['links','show']);
  assert.equal(open.calls().length,count,'read-only rendering/query unexpectedly fetched GitHub');
  open.ok(['links','refresh-if-due']);
  assert.equal(open.calls().length,count,'success TTL failed to deduplicate an immediate refresh');
  const expiry=open.get();expiry.next_refresh_at_unix_utc=now()-1;
  writeFileSync(open.cache(),JSON.stringify(expiry));open.ok(['links','refresh-if-due']);
  assert(open.calls().length>count,'expired metadata did not refresh');

  // Manual issue/PR choices and explicit none are independent of auto discovery.
  const manual=fixtureCase('manual');
  manual.fixture['issue:'+repository+'#143']=issue(143);
  responses(manual,[pr(167,{head:manual.head})]);
  manual.ok(['links','primary','issue','#143']);
  links=manual.refresh();
  assert.equal(links.primary_pr,id('pr',167),'manual issue suppressed automatic PR discovery');
  assert.equal(links.primary_issue,id('issue',143));assert.equal(links.manual_issue,true);
  manual.ok(['links','none','issue']);links=manual.refresh();
  assert.equal(links.issue_not_linked,true);assert.equal(links.primary_issue,null);assert.equal(links.issue_state,'none');
  assert.equal(links.primary_pr,id('pr',167));
  manual.ok(['links','auto','issue']);links=manual.refresh();
  assert.equal(links.primary_issue,id('issue',142));assert.equal(links.manual_issue,false);
  manual.ok(['links','none','pr']);links=manual.refresh();
  assert.equal(links.pr_not_created,true);assert.equal(links.primary_pr,null);assert.equal(links.pr_state,'none');
  manual.ok(['links','auto','pr']);links=manual.refresh();assert.equal(links.primary_pr,id('pr',167));
  const closed=pr(180,{head:manual.head,state:'MERGED',issues:[]});
  manual.fixture['pr:'+repository+'#180']=closed;manual.list([closed]);
  manual.ok(['links','primary','pr','180']);links=manual.refresh();
  assert.equal(links.primary_pr,id('pr',180),'explicit closed PR was replaced by auto discovery');
  assert.equal(links.manual_pr,true);assert.equal(links.entries[id('pr',180)].state,'MERGED');
  assert.notEqual(manual.run(['links','primary','issue','https://github.com/fixture/work-links/issues/142?bad=1']).status,0);

  // No open PR and multiple open PRs are different successful query outcomes.
  const absence=fixtureCase('none');responses(absence,[]);links=absence.refresh();
  assert.equal(links.pr_state,'none');assert.equal(links.primary_pr,null);
  assert.equal(links.discovery_error,null);assert(links.discovery_checked_at_unix_utc);
  const old=pr(180,{head:absence.head,state:'MERGED',issues:[180]});responses(absence,[old]);links=absence.refresh();
  assert.equal(links.pr_state,'none','historical PR must not be promoted automatically');
  assert(links.pr_candidates.includes(id('pr',180)),'historical PR unavailable for manual choice');
  assert.equal(links.primary_issue,null,'historical candidates supplied an automatic issue');
  const ambiguous=fixtureCase('ambiguous');
  responses(ambiguous,[pr(167,{head:ambiguous.head,issues:[142]}),pr(168,{head:ambiguous.head,issues:[143]})]);
  links=ambiguous.refresh();assert.equal(links.pr_state,'ambiguous');assert.equal(links.primary_pr,null);
  assert.equal(links.primary_issue,null);assert.equal(links.issue_state,'unchecked');
  assert.equal(links.issue_candidates.length,0,'ambiguous PR issue references were pooled');

  // Read failures preserve the last good cache, timestamp and an error/backoff.
  const offline=fixtureCase('offline');responses(offline,[pr(167,{head:offline.head})]);
  const cached=offline.refresh();links=offline.refresh({PM_TEST_GH_FAIL:'1'});
  assert.equal(links.pr_state,'error');assert(links.discovery_error);
  assert.equal(links.primary_pr,cached.primary_pr);assert.equal(links.entries[id('pr',167)].title,cached.entries[id('pr',167)].title);
  assert.equal(links.discovery_checked_at_unix_utc,cached.discovery_checked_at_unix_utc);
  assert(links.next_refresh_at_unix_utc>now());assert(links.consecutive_failures>=1);
  const failedCount=offline.calls().length;offline.ok(['links','refresh-if-due'],{PM_TEST_GH_FAIL:'1'});
  assert.equal(offline.calls().length,failedCount,'failure backoff retried immediately');
  links=offline.refresh();assert.equal(links.pr_state,'linked');assert.equal(links.consecutive_failures,0);
  offline.list({__raw:'{invalid json'});links=offline.refresh();
  assert.equal(links.pr_state,'error');assert.equal(links.primary_pr,cached.primary_pr,'malformed result discarded the last successful link');

  // Metadata identities are checked rather than accepting a plausible title.
  const mismatch=fixtureCase('identity');responses(mismatch,[pr(167,{head:mismatch.head})]);
  const good=mismatch.refresh();
  mismatch.ok(['links','primary','pr','167']);
  mismatch.fixture['pr:'+repository+'#167']={...pr(168,{head:mismatch.head}),title:'다른 PR 응답'};mismatch.save();
  links=mismatch.refresh();assert.equal(links.pr_state,'error');assert(links.entries[id('pr',167)].error);
  assert.equal(links.entries[id('pr',167)].title,good.entries[id('pr',167)].title);
  assert(!Object.values(links.entries).some(link=>link.title==='다른 PR 응답'));
  const badList=fixtureCase('list-identity');
  badList.list([{...pr(167,{head:badList.head}),url:'https://github.com/fixture/work-links/pull/168'}]);
  links=badList.refresh();assert.equal(links.primary_pr,null,'mismatched list number/URL linked the wrong PR');
  assert.equal(links.pr_state,'error','invalid identity was described as a successful empty query');
  assert.equal(badList.calls().filter(args=>args[1]==='view').length,0,'invalid list identity was used for follow-up metadata');
  const references=fixtureCase('references');
  responses(references,[pr(167,{head:references.head,issues:[142,143]})]);links=references.refresh();
  assert.equal(links.pr_state,'linked');assert.equal(links.issue_state,'ambiguous');assert.equal(links.primary_issue,null);
  references.fixture['pr:'+repository+'#167']={...pr(167,{head:references.head}),closingIssuesReferences:[{url:'https://example.invalid/issues/142'}]};
  references.list([pr(167,{head:references.head})]);links=references.refresh();
  assert.equal(links.pr_state,'linked','invalid issue refs poisoned an otherwise valid PR identity');
  assert.equal(links.issue_state,'error');assert.equal(links.primary_issue,null);
  const truncated=fixtureCase('list-limit');
  truncated.list(Array.from({length:100},()=>pr(167,{head:truncated.head})));links=truncated.refresh();
  assert.equal(links.pr_state,'error');assert.equal(links.primary_pr,null,'query limit made an unverified unique selection');
  const identityMissing=fixtureCase('source-missing');
  identityMissing.list([{...pr(167,{head:identityMissing.head}),headRepository:null,headRepositoryOwner:null}]);links=identityMissing.refresh();
  assert.equal(links.pr_state,'error');assert.equal(links.primary_pr,null);

  // Changed remotes invalidate automatic associations even on the same branch.
  const changed=fixtureCase('remote-change');responses(changed,[pr(167,{head:changed.head})]);changed.refresh();
  const newRepo='fixture/other-work-links',newPr=pr(188,{head:changed.head,repo:newRepo,issues:[]});
  changed.git('remote','set-url','origin','https://github.com/'+newRepo+'.git');
  changed.fixture['pr:'+newRepo+'#188']=newPr;changed.list([newPr],branchName,newRepo);links=changed.refresh();
  assert.equal(links.repository,newRepo);assert.equal(links.primary_pr,id('pr',188,newRepo));assert.equal(links.primary_issue,null);
  const selectedAfterOrigin=fixtureCase('selection-origin-change');
  responses(selectedAfterOrigin,[pr(167,{head:selectedAfterOrigin.head})]);selectedAfterOrigin.refresh();
  selectedAfterOrigin.git('remote','set-url','origin','https://github.com/'+newRepo+'.git');
  selectedAfterOrigin.fixture['issue:'+newRepo+'#143']=issue(143,newRepo);
  selectedAfterOrigin.fixture['pr:'+newRepo+'#188']=pr(188,{head:selectedAfterOrigin.head,repo:newRepo,issues:[]});
  selectedAfterOrigin.list([selectedAfterOrigin.fixture['pr:'+newRepo+'#188']],branchName,newRepo);
  selectedAfterOrigin.ok(['links','primary','issue',newRepo+'#143']);links=selectedAfterOrigin.get();
  assert.equal(links.primary_pr,null,'manual issue assignment relabeled the previous origin PR as current');
  links=selectedAfterOrigin.refresh({PM_TEST_GH_FAIL:'1'});
  assert.equal(links.primary_pr,null,'failed new-origin query restored the old-origin automatic PR');
  assert.equal(links.primary_issue,id('issue',143,newRepo),'new explicit choice was lost on discovery failure');
  links=selectedAfterOrigin.refresh();assert.equal(links.primary_pr,id('pr',188,newRepo));
  const local=fixtureCase('local-only',{remote:false});local.list([]);links=local.refresh();
  assert.equal(links.repository,null);assert.equal(links.primary_pr,null);assert.equal(local.calls().length,0,'non-GitHub repository invented a query');
  assert.equal(links.discovery_unavailable,true);assert.notEqual(links.pr_state,'none','unavailable source was described as a confirmed absent PR');
  assert.notEqual(local.run(['links','primary','issue','142']).status,0,'bare number accepted without a proven repository');
  local.fixture['issue:'+repository+'#142']=issue(142);local.fixture['pr:'+repository+'#167']=pr(167,{head:local.head});local.save();
  local.ok(['links','primary','issue',repository+'#142']);local.ok(['links','primary','pr',repository+'#167']);links=local.refresh();
  assert.equal(links.primary_issue,id('issue',142));assert.equal(links.primary_pr,id('pr',167));
  assert.equal(links.entries[id('issue',142)].title,'이슈 142');assert.equal(links.entries[id('pr',167)].title,'PR 167');
  assert(!local.calls().some(args=>args[1]==='list'),'manual full refs invented branch discovery on a repo without origin');
  const detached=fixtureCase('detached');detached.git('checkout','--detach');detached.list([]);links=detached.refresh();
  assert.equal(links.discovery_unavailable,true);assert.notEqual(links.pr_state,'none');assert.equal(detached.calls().length,0);

  // Refresh locking must not hold the local-choice lock during network work.
  const race=fixtureCase('choice-race');const racePr=pr(167,{head:race.head});responses(race,[racePr]);
  race.fixture['issue:'+repository+'#143']=issue(143);race.list({__delay_ms:1500,__response:[racePr]});
  const fetching=spawn(binary,['links','refresh'],{env:race.env,stdio:['ignore','ignore','pipe']});
  race.jobs.push(fetching);
  let fetchError='';fetching.stderr.on('data',data=>fetchError+=data.toString());
  await until(()=>race.calls().some(args=>args[1]==='list'),'background fixture query did not start');
  const started=Date.now();race.ok(['links','refresh']);
  assert(Date.now()-started<1000,'duplicate refresh blocked behind network work');
  const saving=Date.now();race.ok(['links','primary','issue','143']);
  assert(Date.now()-saving<1000,'manual selection blocked behind network work');
  await until(()=>fetching.exitCode!==null,'background fixture refresh did not finish');
  assert.equal(fetching.exitCode,0,fetchError);links=race.get();
  assert.equal(links.primary_issue,id('issue',143),'slow automatic response overwrote a newer manual choice');
  assert.equal(links.primary_pr,id('pr',167));assert.equal(links.manual_issue,true);
  assert.equal(race.calls().filter(args=>args[1]==='list').length,1,'concurrent refresh was duplicated');
  race.ok(['links','refresh-if-due']);assert.equal(race.get().entries[id('issue',143)].title,'이슈 143');

  // A live panel shows checking before completing its automatic first query.
  const background=fixtureCase('panel');const panelPr=pr(167,{head:background.head});responses(background,[panelPr]);
  background.ok(['hook'],{},JSON.stringify({hook_event_name:'UserPromptSubmit',session_id:'fixture-session',turn_id:'fixture-panel-turn',cwd:background.repo,prompt:'이 연결 흐름을 점검해 주세요.'}));
  const task=()=>{const state=json(join(background.space,'task-links.json'));return {...state.threads[state.active_thread],revision:state.revision};};
  background.fixture['issue:'+repository+'#143']=issue(143);
  background.list({__delay_ms:1800,__response:[panelPr]});
  const opener=join(background.root,'mock-open.mjs'),openCalls=join(background.root,'open-calls.jsonl');
  writeFileSync(opener,"#!/usr/bin/env node\nimport {appendFileSync} from 'node:fs';\nappendFileSync(process.env.PM_TEST_OPEN_CALLS,JSON.stringify(process.argv.slice(2))+'\\n');\n");
  chmodSync(opener,0o755);background.env.MAEKLOG_OPEN_BIN=opener;background.env.PM_TEST_OPEN_CALLS=openCalls;
  let p=panel(background);
  await until(()=>background.get().pr_state==='checking','initial background checking state missing');
  assert.equal(p.child.exitCode,null,'network query blocked/closed the interactive panel');
  await p.send('vvvv');
  await until(()=>p.text().includes('작업연결확인중'),'pending GitHub lookup was not visible');
  capture(p,'panel-checking');
  await until(()=>background.get().pr_state==='linked'&&background.get().refresh_started_at_unix_utc===null&&background.calls().filter(args=>args[1]==='list').length===2&&p.text().includes('관찰브랜치의연결을다시확인했습니다'),()=>JSON.stringify({error:'explicit refresh while automatic job ran was lost or left pending forever',listCalls:background.calls().filter(args=>args[1]==='list').length,state:background.get().pr_state,frame:p.text().slice(0,1500)}));
  await until(()=>p.text().includes('현재대화의작업'),'main task scope was not shown');
  const oldAliasCalls=background.calls().length;
  await p.send('R');await pause(250);
  assert.equal(background.calls().length,oldAliasCalls,'old uppercase R still refreshed from the base panel');
  assert(!p.text().includes('work-links#167'),'a branch association was promoted to the dialogue task');
  await p.send('l');await until(()=>p.text().includes('작업연결정정')&&p.text().includes('work-links#167'),'automatic branch PR is not accessible as a candidate');
  capture(p,'branch-linked-candidate');await p.send('\x1b');await until(()=>!p.text().includes('작업연결정정'),'branch candidate did not return');
  const stableCalls=background.calls().length;
  const journal=join(background.directory(),'journal.json');
  const beforeJournal=existsSync(journal)?readFileSync(journal,'utf8'):null;
  background.ok(['render-once']);background.ok(['render-history']);
  await p.send('h');await until(()=>p.text().includes('고정된내용')&&p.text().includes('전체기록·읽기전용'),'full records reader did not open');
  await p.send('\x1b');await until(()=>!p.text().includes('고정된내용')&&p.text().includes('저장된기록'),'full records reader did not return');
  await p.send('e');await until(()=>p.text().includes('Enter근거'),'record browser did not open');
  await p.send('\x1b');await until(()=>!p.text().includes('Enter근거')&&p.text().includes('저장된기록'),()=>JSON.stringify({error:'record browser did not return to the main panel',frame:p.text()}));
  await p.send('b');await until(()=>p.text().includes('날짜를선택하세요'),'date browser did not open');
  await p.send('\x1b');await until(()=>!p.text().includes('날짜를선택하세요')&&p.text().includes('저장된기록'),()=>JSON.stringify({error:'date browser did not return to the main panel',frame:p.text()}));
  await p.send('\x1b[21~');
  await until(()=>p.text().includes('연결상태새로고침'),()=>JSON.stringify({error:'connection action missing from F10 menu',exitCode:p.child.exitCode,frame:p.text().slice(0,2000)}));
  capture(p,'panel-menu');
  await p.send('\x1b');await until(()=>!p.text().includes('연결상태새로고침')&&p.text().includes('저장된기록'),'F10 menu did not close');
  p.child.kill('SIGUSR1');await pause(150);p.child.kill('SIGUSR2');await pause(150);
  assert.equal(background.calls().length,stableCalls,'history/menu/editor/date/resize browsing fetched GitHub');
  assert.equal(existsSync(journal)?readFileSync(journal,'utf8'):null,beforeJournal,'connection browsing rewrote conversation records');

  // The chooser scopes numeric input, persists choices, opens one canonical URL,
  // cancels edits and restores the previous dialogue choice without a model.
  await p.send('l');await until(()=>p.text().includes('작업연결정정'),'connection chooser did not open');
  await p.send('a');
  await until(()=>p.text().includes('번호만입력해도'),'connection input did not open');
  capture(p,'chooser-input');
  p.child.kill('SIGALRM');await pause(250);capture(p,'chooser-input-narrow');
  p.child.kill('SIGUSR2');await pause(250);
  await p.send('\x1b[200~#143\x1b[201~\r');
  await until(()=>task().current?.[0]?.id===repository+'#143'&&task().current[0].link?.title==='이슈 143'&&!p.text().includes('작업연결정정'),'numeric manual assignment did not finish');
  assert.equal(task().current[0].provenance,'manual');
  assert.equal(background.get().primary_issue,id('issue',142),'dialogue selection rewrote branch association');
  capture(p,'chooser-manual');
  await p.send('o');await until(()=>lines(openCalls).length===1,'selected connection did not reach the mock opener');
  assert.deepEqual(lines(openCalls),[[`https://github.com/${repository}/issues/143`]],'opener did not receive exactly one canonical URL argument');
  await p.send('\x1b');await until(()=>!p.text().includes('작업연결정정')&&p.text().includes('저장된기록'),'opener result did not return to the main panel');
  await p.send('l');await until(()=>p.text().includes('작업연결정정'),'none chooser did not open');
  capture(p,'none-before');if(artifacts)writeFileSync(join(artifacts,'none-before.json'),JSON.stringify(task(),null,2));
  await p.send('n');await until(()=>p.text().includes('이작업의연결을해제합니다'),'none row was not selected');
  assert.equal(task().explicit_none,false,'none navigation saved without Enter');
  capture(p,'none-navigation');if(artifacts)writeFileSync(join(artifacts,'none-navigation.json'),JSON.stringify(task(),null,2));
  await p.send('\r');await until(()=>task().explicit_none===true&&!p.text().includes('작업연결정정'),'explicit none was not saved');
  capture(p,'none-after');if(artifacts)writeFileSync(join(artifacts,'none-after.json'),JSON.stringify(task(),null,2));
  await p.send('l');await until(()=>p.text().includes('작업연결정정'),'undo chooser did not open');await p.send('u');
  await until(()=>task().current?.[0]?.id===repository+'#143'&&!p.text().includes('작업연결정정'),'undo failed to restore previous dialogue target');
  const cancelBefore=task().revision,cancelCalls=background.calls().length;
  await p.send('l');await until(()=>p.text().includes('작업연결정정'),'cancel chooser did not open');
  await p.send('a');await until(()=>p.text().includes('번호만입력해도'),'cancelled assignment form did not open');
  await p.send('vrR');await until(()=>p.text().includes('vrR'),'refresh letters were swallowed in typed input');
  assert.equal(background.calls().length,cancelCalls,'typing refresh letters fetched GitHub');
  await p.send('\x1b[200~#144\x1b[201~');await p.send('\x1b');
  await until(()=>!p.text().includes('번호만입력해도')&&p.text().includes('작업연결정정'),'assignment form did not cancel');
  await p.send('\x1b');await until(()=>!p.text().includes('작업연결정정')&&p.text().includes('저장된기록'),'connection chooser did not close');
  assert.equal(task().revision,cancelBefore,'cancelled input saved a connection');
  assert.equal(background.calls().length,cancelCalls,'opening/cancelling connection input fetched GitHub');
  await p.send('l');await until(()=>p.text().includes('작업연결정정'),'refresh chooser did not open');
  const forcedBefore=background.calls().filter(args=>args[0]==='api').length;
  await p.send('rR');await pause(250);
  assert.equal(background.calls().filter(args=>args[0]==='api').length,forcedBefore,'old link-picker r/R aliases still refreshed');
  await p.send('vvvv');
  await until(()=>background.calls().filter(args=>args[0]==='api').length>forcedBefore&&task().current[0].verification==='verified','chooser refresh key did not complete');
  assert.equal(background.calls().filter(args=>args[0]==='api').length,forcedBefore+1,'repeated refresh keys duplicated an in-flight fetch');
  await closePanel(p);
  const reopenedCount=background.calls().length;p=panel(background);
  await until(()=>p.text().includes('work-links#143'),'reopened cached panel did not render');await pause(1200);
  assert.equal(background.calls().length,reopenedCount,'reopening a fresh cache repeated the metadata request');
  await closePanel(p);

  const io=fixtureCase('io-error');io.list([]);io.ok(['links','auto','issue']);
  mkdirSync(join(io.directory(),'.links-fetch.lock'));
  const failedPanel=panel(io);
  await until(()=>failedPanel.text().includes('작업연결')&&failedPanel.text().includes('실패'),'automatic I/O failure before cache write was hidden');
  capture(failedPanel,'panel-local-error');
  assert.equal(failedPanel.child.exitCode,null);assert.equal(io.calls().length,0);
  await closePanel(failedPanel);

  for(const [c,label,reference] of [[absence,'panel-none','work-links#180'],[local,'panel-manual-no-origin','work-links#167'],[offline,'panel-query-error','work-links#167'],[ambiguous,'panel-ambiguous','work-links#168']]){
    const shown=panel(c);await until(()=>shown.text().includes('현재대화의작업')&&shown.text().includes('진행정보없음'),label+' did not render');
    assert(!shown.text().includes(reference),'historical branch reference looked like a dialogue target');
    capture(shown,label);await shown.send('l');await until(()=>shown.text().includes('작업연결정정')&&shown.text().includes(reference),label+' branch reference is inaccessible');
    assert(shown.text().includes('지금은참조보기만'),'unbound branch picker permits task mutations');
    capture(shown,label+'-references');await closePanel(shown);
  }
  assert(offline.ok(['render-history']).includes('조회 실패'),'old cache failure was lost from the full branch view');
  const unavailable=fixtureCase('panel-unavailable',{remote:false});unavailable.list([]);
  const unavailablePanel=panel(unavailable);
  await until(()=>unavailablePanel.text().includes('현재대화의작업')&&unavailablePanel.text().includes('진행정보없음'),'unavailable repository did not keep the quiet unassigned task card');
  assert(!unavailablePanel.text().includes('열린PR없음'),'unavailable repository looked like a confirmed absent PR');
  capture(unavailablePanel,'panel-unavailable');await closePanel(unavailablePanel);

  // A delayed branch response cannot appear after checkout. A dialogue choice
  // stays bound to its proven repository even as the branch observation changes.
  const switching=fixtureCase('checkout');const first=pr(167,{head:switching.head});responses(switching,[first]);
  switching.list({__delay_ms:1500,__response:[first]});
  const otherBranch='feature/other-links',second=pr(168,{head:switching.head,branch:otherBranch,issues:[143]});
  switching.ok(['hook'],{},JSON.stringify({hook_event_name:'UserPromptSubmit',session_id:'fixture-session',turn_id:'fixture-switch-turn',cwd:switching.repo,prompt:'현재 연결 후보를 점검해 주세요.'}));
  switching.fixture['issue:'+repository+'#144']=issue(144);
  switching.fixture['pr:'+repository+'#168']=second;switching.fixture['issue:'+repository+'#143']=issue(143);switching.list([second],otherBranch);
  const switched=panel(switching);
  await until(()=>switching.calls().some(args=>args[1]==='list'),'delayed old-branch lookup did not start');
  switching.git('checkout','-b',otherBranch);
  await until(()=>switching.get().primary_pr===id('pr',168),'new branch lookup did not complete');
  assert(!switched.text().includes('work-links#167'),'old-branch connection leaked into current screen');
  assert.equal(json(join(switching.directory(branchName),'links.json')).primary_pr,null,'checkout retained a response fetched for an obsolete context');
  await switched.send('l');await until(()=>switched.text().includes('작업연결정정'),'checkout chooser did not open');
  await until(()=>switched.text().includes('work-links#168')&&!switched.text().includes('work-links#167'),'checkout chooser mixed old branch candidates');
  await switched.send('a');await until(()=>switched.text().includes('번호만입력해도'),'checkout form did not open');await switched.send('\x1b[200~#144\x1b[201~');
  const otherRevision=switching.get().selection_revision;
  switching.git('checkout',branchName);await switched.send('\r');
  await until(()=>{const state=json(join(switching.space,'task-links.json'));const current=state.threads[state.active_thread].current?.[0];return current?.id===repository+'#144'&&current.verification==='verified'&&!switched.text().includes('작업연결정정');},'dialogue assignment lost its frozen repository across checkout');
  capture(switched,'checkout-assigned');if(artifacts)writeFileSync(join(artifacts,'checkout-assigned.json'),readFileSync(join(switching.space,'task-links.json')));
  const otherCache=json(join(switching.directory(otherBranch),'links.json'));
  assert.equal(otherCache.selection_revision,otherRevision);assert(!otherCache.entries[id('issue',144)]);
  await until(()=>switching.get().refresh_started_at_unix_utc===null,'last fixture fetch did not settle');
  await closePanel(switched);

  for(const c of cases)assertReadOnly(c);
  assert.equal(createHash('sha256').update(readFileSync(new URL(import.meta.url))).digest('hex'),scriptSha,'fixture changed while running');
  if(artifacts)putRun({passed:true});
  console.log('PASS work links: branch/source identity, selected-PR issue, manual/none/auto and history choices, no-match/ambiguity/error/cache, TTL/backoff/concurrent deduplication, real PTY automatic pending lookup, pure navigation, scoped manual input/open/cancel/refresh and checkout isolation');
}catch(error){
  if(artifacts){putRun({passed:false,error:String(error.stack??error)});for(const c of cases)if(c.panel){capture(c.panel,'failure-'+c.name);const path=join(c.space,'task-links.json');if(existsSync(path))writeFileSync(join(artifacts,'failure-'+c.name+'.json'),readFileSync(path));}}
  throw error;
}finally{
  for(const c of cases){
    if(c.child&&c.child.exitCode===null){c.child.stdin.write('\x1b\x1bq');await pause(300);if(c.child.exitCode===null)c.child.kill('SIGTERM');}
    for(const job of c.jobs)if(job.exitCode===null)job.kill('SIGTERM');
    rmSync(c.root,{recursive:true,force:true});
  }
}
