// Real worker, mock reducer/GitHub/Herdr, real PTY. No model or network requests.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,readdirSync,existsSync,rmSync,chmodSync} from 'node:fs';
import {spawnSync,spawn} from 'node:child_process';
import {resolve,join,basename} from 'node:path';
import {tmpdir} from 'node:os';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const artifacts=process.argv[3]?resolve(process.argv[3]):null;
const root=mkdtempSync(join(tmpdir(),'pm-branch-integration-')),repo=join(root,'demo-shop'),state=join(root,'state');
mkdirSync(repo);
const git=(...args)=>{const r=spawnSync('git',args,{cwd:repo,encoding:'utf8'});assert.equal(r.status,0,r.stderr);return r.stdout.trim();};
git('init','-b','feature/shower-guide');git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','--allow-empty','-m','fixture');
git('remote','add','origin','https://github.com/fixture/demo-shop.git');
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_WORKSPACE_ID:'fixture',HERDR_PANE_ID:'fixture:pm',HERDR_SESSION:'branch-test',
HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),PM_TEST_ROOT:root,PM_TEST_BINARY:binary,MAEKLOG_CODEX_BIN:resolve('scripts/mock-branch-reducer.mjs'),
MAEKLOG_GH_BIN:resolve('scripts/mock-github.mjs'),PM_TEST_CALLS:join(root,'model-calls.jsonl'),PM_TEST_DELTAS:join(root,'deltas.json'),PM_TEST_GH_CALLS:join(root,'gh-calls.jsonl'),PM_TEST_GH_FIXTURE:join(root,'github.json'),TZ:'Asia/Seoul',TERM:'xterm-256color'};
delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_DISABLE_LUNA;delete env.PM_TEST_GH_FAIL;
delete env.NO_COLOR; // Explicit color fixture; the invoking agent may disable ANSI globally.
for(const path of [env.HERDR_BIN_PATH,env.MAEKLOG_CODEX_BIN,env.MAEKLOG_GH_BIN])chmodSync(path,0o755);
const run=(args,extra={},input)=>spawnSync(binary,args,{env:{...env,...extra},input,encoding:'utf8',timeout:15000});
const ok=(args,extra={},input)=>{const r=run(args,extra,input);assert.equal(r.status,0,JSON.stringify({args,error:r.error?{code:r.error.code,message:r.error.message}:null,signal:r.signal,stderr:r.stderr?.slice(-2000),stdout:r.stdout?.slice(-2000)}));return r.stdout;};
const lines=path=>existsSync(path)?readFileSync(path,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
const json=path=>JSON.parse(readFileSync(path,'utf8'));
const put=(path,data)=>writeFileSync(path,JSON.stringify(data));
const hash=value=>{let h=0xcbf29ce484222325n;for(const byte of Buffer.from(value)){h^=BigInt(byte);h=BigInt.asUintN(64,h*0x100000001b3n);}return h.toString(16).padStart(16,'0');};
const recordId=(kind,key,stage)=>`${kind==='Decision'?'D':'C'}-${hash(kind+':'+key+':'+stage)}`;
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(check,message){const deadline=Date.now()+10000;while(Date.now()<deadline){if(check())return;await pause(30);}throw Error(message);}
let child;
try{
  ok(['hook'],{},JSON.stringify({hook_event_name:'SessionStart',session_id:'fixture-session',cwd:repo}));
  const spaceKey=readdirSync(join(state,'spaces'))[0],space=join(state,'spaces',spaceKey);
  ok(['links','primary','issue','142']); // The displayed/proven GitHub repository scopes a bare number.
  ok(['links','primary','issue','fixture/demo-shop#142']);ok(['links','primary','pr','fixture/demo-shop#167']);
  const branchKey=readdirSync(join(state,'branches'))[0],branch=join(state,'branches',branchKey),scope=json(join(branch,'context.json'));
  const messages=['特定モデル説明も含めると確定する','資料整理完了','特定モデルだけの説明は除外すると変更する','범위 조정 완료','배송·반품 안내는 이번 작업에서 수정하지 않는다','가이드 초안 작성 완료','초안을 점검해 줘','문구 수정 완료','조건 누락 여부를 검증해 줘','검증에서 누락 발견, 재작업 필요','수정안을 준비해 줘','누락 보완 후 재검증 통과','수정안을 검토할게','가이드 문구 수정안 작성 완료'];
  messages[0]='특정 모델 설명도 포함하기로 한다';messages[1]='자료 정리 완료';messages[2]='특정 모델에만 해당하는 설명은 제외한다';
  messages[13]='가이드 문구 수정안 작성 완료. 수정안 작성 후 사용자 검토 단계. 실제 반영 전에 사용자 승인이 필요하다. 사용자 — 수정안을 검토하고 반영 여부를 결정한다. 승인되면 구현 담당이 반영하고 검증한다.';
  const base=Date.parse(JSON.parse(ok(['daily','list'])).today+'T00:00:00+09:00')/1000;
  const events=messages.map((content,i)=>({seq:i+1,kind:i%2?'assistant_final':'user_prompt',session_id:'fixture-session',turn_id:'turn-'+Math.floor(i/2),content,truncated:false,recorded_at_unix:base+(i+1)*60,cwd:repo}));
  put(join(space,'event-sources.json'),Object.fromEntries(events.map(e=>[e.seq,{uid:'fixture-'+e.seq,order:e.seq,space:spaceKey,session:e.session_id,turn:e.turn_id,kind:e.kind,at_unix_utc:e.recorded_at_unix||null,context:scope}])));
  writeFileSync(join(space,'inbox.jsonl'),events.map(JSON.stringify).join('\n')+'\n');put(join(space,'sequence.json'),{next_seq:15});
  put(join(space,'connection.json'),{...json(join(space,'connection.json')),last_event:'Stop'});
  const defs={};
  const decision=(key,text,operation='add',target=null)=>({kind:'decision',operation,key,target,text,condition:null,stage:'decision',outcome:'confirmed'});
  defs[1]={records:[decision('model-scope-original',messages[0])]};
  defs[3]={records:[decision('model-scope',messages[2],'replace',recordId('Decision','model-scope-original','Decision'))]};
  defs[5]={records:[decision('shipping-excluded',messages[4])]};
  for(const e of events.filter(e=>e.kind==='assistant_final')){
    const target=e.seq===10?recordId('Completion','completion-8','Implementation'):e.seq===12?recordId('Completion','completion-10','Verification'):null;
    defs[e.seq]={records:[{kind:'completion',operation:target?'follow_up':'add',key:'completion-'+e.seq,target,text:e.content,condition:null,stage:[10,12].includes(e.seq)?'verification':'implementation',outcome:e.seq===10?'failed':e.seq===12?'passed':'reported'}]};
  }
  defs[14].states=[
    {key:'main',kind:'progress',text:'수정안 작성 후 사용자 검토 단계',owner:'codex',condition:null,status:'active',change:'changed'},
    {key:'approval',kind:'blocker',text:'실제 반영 전에 사용자 승인이 필요하다',owner:'user',condition:'사용자 승인 후 실제 반영',status:'active',change:'changed'},
    {key:'review',kind:'action',text:'수정안을 검토하고 반영 여부를 결정한다',owner:'user',condition:'승인되면 구현 담당이 반영하고 검증한다',status:'active',change:'changed'}
  ];
  defs[14].records[0].text='가이드 문구 수정안 작성 완료';
  put(env.PM_TEST_DELTAS,defs);
  ok(['worker']);
  const journalPath=join(branch,'journal.json'),journal=json(journalPath);
  assert.equal(Object.keys(journal.records).length,10); // three decision versions represented by 3 records + 7 completion reports
  assert.equal(Object.values(journal.records).filter(r=>r.kind==='decision'&&!r.replaced_by&&!r.withdrawn).length,2);
  assert.equal(journal.records[recordId('Completion','completion-2','Implementation')].versions[0].at_unix_utc,base+120);
  const calls=lines(env.PM_TEST_CALLS);assert.equal(calls.length,2,'two newest-first cumulative calls; no duplicate preview call');
  assert(calls.every(c=>c.events.length<=8));
  assert(calls.every(c=>c.args.includes('gpt-6-luna')&&c.args.includes('model_reasoning_effort="medium"')&&c.args.includes('service_tier="default"')&&c.args.includes('features.fast_mode=false')&&c.args.includes('read-only')));
  const before=readFileSync(journalPath,'utf8');ok(['worker']);assert.equal(readFileSync(journalPath,'utf8'),before);assert.equal(lines(env.PM_TEST_CALLS).length,2);
  const gh={
    'issue:fixture/demo-shop#142':{number:142,url:'https://github.com/fixture/demo-shop/issues/142',title:'공용 구매 가이드 개선',body:'## 문제와 원하는 결과\n고객이 수전의 차이를 이해하도록 공용 구매 가이드를 개선한다.',state:'OPEN'},
    'pr:fixture/demo-shop#167':{number:167,url:'https://github.com/fixture/demo-shop/pull/167',title:'공용 가이드 문구와 구성 수정',body:'## 변경 범위\n공용 가이드의 문구와 구성을 수정한다.\n## 검증\n본문 확인은 diff 검증이 아니다.',headRefName:scope.branch,headRefOid:scope.head,state:'OPEN',headRepository:{name:'demo-shop',nameWithOwner:'fixture/demo-shop'},headRepositoryOwner:{login:'fixture'},isCrossRepository:false,closingIssuesReferences:[{number:142,url:'https://github.com/fixture/demo-shop/issues/142'}]},
    list:[]
  };
  gh.list.push({...gh['pr:fixture/demo-shop#167']});
  put(env.PM_TEST_GH_FIXTURE,gh);ok(['links','refresh']);
  const cached=json(join(branch,'links.json'));
  ok(['links','refresh'],{PM_TEST_GH_FAIL:'1'});
  const failed=json(join(branch,'links.json'));
  assert.equal(failed.entries['Pr:fixture/demo-shop#167'].checked_at_unix_utc,cached.entries['Pr:fixture/demo-shop#167'].checked_at_unix_utc);
  assert(failed.entries['Pr:fixture/demo-shop#167'].error);
  assert(ok(['render-history']).includes('조회 실패'),'full branch view lost its failed metadata cache');
  ok(['links','refresh']);
  const basic=ok(['render-once']),history=ok(['render-history']);
  assert(basic.includes('오늘 완료 7건'));assert(!basic.includes('시각 미확인  자료 정리'));
  assert(history.includes('결정 변경 이력'));
  for (const event of events.filter(e=>e.kind==='assistant_final')) {
    assert(history.replaceAll('\n','').includes(event.content.split('.')[0]), 'full history lost a today completion report');
  }
  assert(basic.replace(/\s/g,'').includes('승인되면구현담당이반영하고검증한다'));
  assert(history.includes('본문 발췌'));assert(!basic.includes('완료 후 후속 문제'));
  const unwrappedBasic=basic.replace(/\s/g,''),exactText=text=>text.replace(/\s/g,'');
  assert(unwrappedBasic.includes(exactText(messages[4]))&&unwrappedBasic.includes(exactText(messages[2])));
  assert(unwrappedBasic.indexOf(exactText(messages[4])) < unwrappedBasic.indexOf(exactText(messages[2])),'newer active decision must appear first');
  if(artifacts){mkdirSync(artifacts,{recursive:true});writeFileSync(join(artifacts,'panel-default.txt'),basic);writeFileSync(join(artifacts,'panel-history.txt'),history);}
  // Real h/scroll/resize, no model/GitHub calls and no journal writes.
  writeFileSync(join(root,'fixture-panel'),'fixture');
  let output='';child=spawn('python3',[resolve('scripts/pty-history.py'),binary],{env,stdio:['pipe','pipe','pipe']});
  child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
  child.stdout.on('data',s=>output+=s.toString());child.stderr.on('data',s=>output+=s.toString());
  const frame=()=>output.slice(output.lastIndexOf('\x1b[2J'));
  const flattened=()=>frame().replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
  await until(()=>flattened().includes('m메뉴')&&flattened().includes('저장된기록'),'default PTY frame missing');
  const stable=readFileSync(journalPath,'utf8'),modelCount=lines(env.PM_TEST_CALLS).length,ghCount=lines(env.PM_TEST_GH_CALLS).length;
  if(artifacts)writeFileSync(join(artifacts,'panel-default.ansi'),frame());
  assert(frame().includes('\x1b[38;2;235;219;178m\x1b[1m현재 대화의 작업'), 'current section must use neutral bold');
  assert(frame().includes('\x1b[38;2;254;128;25m\x1b[1m'), 'actual stored blocker warning lost its accent');
  assert(frame().includes('\x1b[38;2;235;219;178m\x1b[22m'), 'body must remain neutral');
  // SGR left down/drag/release: select four header characters, copy via OSC 52,
  // retain the selected snapshot, then dismiss without closing the panel.
  const beforeCopy=output.length;
  child.stdin.write('\x1b[<0;1;1M\x1b[<32;4;1M');
  await until(()=>flattened().includes('드래그선택중'),'drag selection missing');
  assert(frame().includes('\x1b[7m'),'selected cells not highlighted');
  if(artifacts)writeFileSync(join(artifacts,'panel-selection.ansi'),frame());
  child.stdin.write('\x1b[<0;4;1m');
  await until(()=>output.slice(beforeCopy).includes('\x1b]52;c;'),'clipboard request missing');
  const encoded=output.slice(beforeCopy).match(/\x1b\]52;c;([^\x07]+)\x07/)[1];
  assert.equal(Buffer.from(encoded,'base64').toString('utf8'),basename(repo).slice(0,4));
  await until(()=>flattened().includes('선택유지'),'released selection lost');
  child.stdin.write('\x1b');
  await until(()=>!flattened().includes('선택유지')&&flattened().includes('m메뉴'),'Esc did not clear selection');
  assert.equal(child.exitCode,null,'Esc selection must not hide the pane');
  assert.equal(readFileSync(journalPath,'utf8'),stable);assert.equal(lines(env.PM_TEST_CALLS).length,modelCount);assert.equal(lines(env.PM_TEST_GH_CALLS).length,ghCount);
  child.stdin.write('h');await until(()=>flattened().includes('전체기록·읽기전용')&&flattened().includes('고정된내용'),'h did not open full records');
  if(artifacts)writeFileSync(join(artifacts,'panel-history.ansi'),frame());
  child.kill('SIGUSR1');await until(()=>frame().includes('\x1b[110;1H')&&flattened().includes('Esc뒤로'),'narrow resize was not observed');
  if(artifacts)writeFileSync(join(artifacts,'panel-narrow.ansi'),frame());
  child.stdin.write('\x1b[F');await until(()=>flattened().includes('자료정리완료'),'End did not reach the earlier completion in full records after narrow resize');
  child.kill('SIGUSR2');await until(()=>flattened().includes('Esc메인으로'),'wide resize was not observed');
  child.stdin.write(']');await until(()=>flattened().includes('유효결정·2건'),'decision category inaccessible');
  child.stdin.write('h');await until(()=>!flattened().includes('고정된내용')&&flattened().includes('m메뉴'),'h did not return to main');
  assert.equal(readFileSync(journalPath,'utf8'),stable);assert.equal(lines(env.PM_TEST_CALLS).length,modelCount);assert.equal(lines(env.PM_TEST_GH_CALLS).length,ghCount);
  // Real editor: read-only evidence, Unicode pasted input, explicit preview/save,
  // append-only exclusion/undo and no new model/GitHub requests.
  child.kill('SIGWINCH');await until(()=>frame().includes('\x1b[42;1H'),'editor height resize failed');
  child.stdin.write('e');await until(()=>flattened().includes('근거보기·사용자정정'),'record list missing');
  if(artifacts)writeFileSync(join(artifacts,'records-list.ansi'),frame());
  child.stdin.write('\r');await until(()=>flattened().includes('저장된원문인용'),'evidence view missing');
  assert.equal(readFileSync(journalPath,'utf8'),stable);
  if(artifacts)writeFileSync(join(artifacts,'record-evidence.ansi'),frame());
  child.stdin.write('e');await until(()=>flattened().includes('내용·조건정정'),'edit form missing');
  const paste=text=>'\x1b[200~'+text+'\x1b[201~';
  child.stdin.write('\x15'+paste('검증 대기 상태로 정정')+'\t'+paste('실제 화면 확인 후')+'\t'+paste('보고 표현을 바로잡음'));
  await until(()=>flattened().includes('보고표현을바로잡음'),'Korean paste missing');
  child.stdin.write('\r');await until(()=>flattened().includes('저장전확인'),'confirmation missing');
  assert(flattened().includes('모든Space에반영'));
  assert.equal(readFileSync(journalPath,'utf8'),stable,'preview wrote journal');
  if(artifacts)writeFileSync(join(artifacts,'correction-confirm.ansi'),frame());
  child.kill('SIGALRM');await until(()=>flattened().includes('변경후')&&frame().includes('\x1b[42;1H')&&!frame().includes('같은 저장소·브랜치 기록을 공유하는 모든 Space'),'confirm resize failed');
  await pause(300);
  if(artifacts)writeFileSync(join(artifacts,'correction-narrow.ansi'),frame());
  child.kill('SIGWINCH');await pause(300);
  child.stdin.write('\r');await until(()=>flattened().includes('저장했습니다'),'correction save failed');
  let amended=json(journalPath);const edited=Object.values(amended.records).find(r=>r.corrections?.length);
  assert(edited);assert.equal(edited.corrections[0].after.text,'검증 대기 상태로 정정');
  assert.equal(edited.corrections[0].after.condition,'실제 화면 확인 후');
  assert.deepEqual(edited.versions,JSON.parse(stable).records[edited.id].versions);
  child.stdin.write('x');await until(()=>flattened().includes('오기록으로제외'),'exclude form missing');
  child.stdin.write(paste('다른 작업의 완료였다')+'\r');await until(()=>flattened().includes('저장전확인'),'exclude preview missing');
  child.stdin.write('\x1b');await pause(200);child.stdin.write('\x1b');await pause(200);
  assert.equal(json(journalPath).records[edited.id].corrections.length,1,'cancel persisted exclusion');
  child.stdin.write('x');await pause(200);child.stdin.write(paste('다른 작업의 완료였다')+'\r');await pause(200);child.stdin.write('\r');
  await until(()=>json(journalPath).records[edited.id].corrections.length===2,'exclude did not commit');
  assert.equal(json(journalPath).records[edited.id].corrections[1].after.excluded,true);
  child.stdin.write('u');await pause(200);child.stdin.write(paste('제외만 되돌림')+'\r');await pause(200);child.stdin.write('\r');
  await until(()=>json(journalPath).records[edited.id].corrections.length===3,'undo did not commit');
  assert.equal(json(journalPath).records[edited.id].corrections[2].after.excluded,false);
  assert.equal(json(journalPath).records[edited.id].corrections[2].undo_of,2);
  assert.equal(lines(env.PM_TEST_CALLS).length,modelCount);assert.equal(lines(env.PM_TEST_GH_CALLS).length,ghCount);
  if(artifacts)writeFileSync(join(artifacts,'correction-history.ansi'),frame());
  child.kill('SIGUSR2');await pause(300);
  child.stdin.write('\x1b');await pause(200);child.stdin.write('\x1b');await until(()=>!flattened().includes('근거보기·사용자정정')&&flattened().includes('m메뉴'),'editor did not close');
  assert(flattened().includes('검증대기상태로정정'));
  child.stdin.write('q');await until(()=>child.exitCode!==null,'panel did not exit');assert.equal(child.exitCode,0);
  // Explicit not-created, multiple candidates, unique source-repository OPEN branch match.
  ok(['links','none','pr']);assert.equal(json(join(branch,'links.json')).pr_not_created,true);
  assert(!ok(['render-once']).includes('PR fixture/demo-shop#167'),'explicit branch none kept displaying the prior PR');
  put(join(branch,'links.json'),{});gh.list.push({...gh.list[0],number:168,url:'https://github.com/fixture/demo-shop/pull/168'});
  gh['pr:fixture/demo-shop#168']={...gh['pr:fixture/demo-shop#167'],number:168,url:'https://github.com/fixture/demo-shop/pull/168'};put(env.PM_TEST_GH_FIXTURE,gh);
  ok(['links','refresh']);assert.equal(json(join(branch,'links.json')).primary_pr,null);assert.equal(json(join(branch,'links.json')).pr_state,'ambiguous');
  assert(!ok(['render-once']).includes('PR fixture/demo-shop#167'),'ambiguous branch candidates looked current');
  put(join(branch,'links.json'),{});gh.list.pop();put(env.PM_TEST_GH_FIXTURE,gh);ok(['links','refresh']);
  assert.equal(json(join(branch,'links.json')).primary_pr,'Pr:fixture/demo-shop#167');
  assert(lines(env.PM_TEST_GH_CALLS).every(a=>['view','list'].includes(a[1])));
  assert.notEqual(run(['memory','clear',branchKey]).status,0);
  assert.notEqual(run(['memory','clear',branchKey,'--confirm-shared',branchKey]).status,0,'enabled shared deletion accepted');
  console.log('PASS branch memory: real v3 mock reduction, preview/cumulative dedup, retained decisions/completions, exact source times, read-only GitHub/cache failure/ambiguity, real h/scroll/resize purity, evidence/amend/exclude/undo/cancel/Unicode paste and explicit deletion guards');
}finally{
  if(child&&child.exitCode===null){child.stdin.write('q');await pause(400);if(child.exitCode===null)child.kill('SIGTERM');}
  rmSync(root,{recursive:true,force:true});
}
