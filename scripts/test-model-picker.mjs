// Real crossterm PTY; only isolated mock Codex model/list calls and local files.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync,chmodSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {spawn,spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const evidence=resolve(process.argv[3]??mkdtempSync(join(tmpdir(),'maeklog-picker-evidence-')));
mkdirSync(evidence,{recursive:true});
const digest=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const checks=[],captures=[];
const result=extra=>writeFileSync(join(evidence,'result.json'),JSON.stringify({candidate_sha256:digest(binary),script_sha256:digest(new URL(import.meta.url)),checks,captures,...extra},null,2));
const compact=s=>s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
const pause=ms=>new Promise(done=>setTimeout(done,ms));
async function until(check,message,timeout=11000){const end=Date.now()+timeout;while(Date.now()<end){if(check())return;await pause(20);}throw Error(typeof message==='function'?message():message);}
const models=[
 {id:'fixture-choice',model:'fixture-choice',displayName:'Fixture 원문 model',supportedReasoningEfforts:[{reasoningEffort:'high',description:'Returned description · 원문 high'},{reasoningEffort:'max',description:'Returned description · 원문 max'}],defaultReasoningEffort:'high',hidden:false,isDefault:true,inputModalities:['text']},
 {id:'fixture-no-effort',model:'fixture-no-effort',displayName:'Fixture no effort',supportedReasoningEfforts:[],defaultReasoningEffort:null,hidden:false,isDefault:false,inputModalities:['text']},
 ...Array.from({length:36},(_,n)=>({id:'fixture-other-'+n,model:'fixture-other-'+n,displayName:'Fixture long model '+n+' · '+'한글🐱'.repeat(25),supportedReasoningEfforts:[{reasoningEffort:'low',description:'Fixture returned effort'}],defaultReasoningEffort:'low',hidden:false,isDefault:false,inputModalities:['text']})),
];
result({passed:false});
for(const locale of ['ko_KR.UTF-8','en_US.UTF-8']){
 const ko=locale.startsWith('ko'),tag=ko?'ko':'en',t=(a,b)=>ko?a:b;
 const root=mkdtempSync(join(tmpdir(),'maeklog-picker-')),state=join(root,'state'),config=join(root,'config'),home=join(root,'home'),codexHome=join(home,'.codex');
 for(const p of [state,config,home,codexHome])mkdirSync(p,{recursive:true});
 const settingsPath=join(config,'model-settings.json'),globalPath=join(codexHome,'config.toml'),fixture=join(root,'catalog.json'),callsPath=join(root,'catalog-calls.jsonl');
 const initial={model:'fixture-unlisted',effort:'max'};
 const setMode=(mode,delay_ms=0)=>writeFileSync(fixture,JSON.stringify({mode,delay_ms,models}));
 writeFileSync(settingsPath,JSON.stringify(initial));writeFileSync(globalPath,'# Global Codex fixture: must remain byte-for-byte unchanged\nmodel = "global-fixture"\n');setMode('ok',1400);
 const env={...process.env,LC_ALL:locale,LANG:ko?'en_US.UTF-8':'ko_KR.UTF-8',LC_MESSAGES:ko?'en_US.UTF-8':'ko_KR.UTF-8',HOME:home,CODEX_HOME:codexHome,XDG_CONFIG_HOME:join(root,'xdg'),HERDR_PLUGIN_CONFIG_DIR:config,
  HERDR_WORKSPACE_ID:'fixture',HERDR_PANE_ID:'fixture:pm',HERDR_SESSION:'model-picker-'+tag,MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),
  HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),PM_TEST_ROOT:root,PM_TEST_BINARY:binary,MAEKLOG_DISABLE_LUNA:'1',MAEKLOG_CODEX_BIN:resolve('scripts/mock-model-picker-codex.mjs'),
  MAEKLOG_PICKER_CALLS:callsPath,MAEKLOG_PICKER_FIXTURE:fixture,TERM:'xterm-256color',COLORTERM:'truecolor',TZ:'Asia/Seoul'};
 delete env.MAEKLOG_INTERNAL;delete env.NO_COLOR;
 chmodSync(env.HERDR_BIN_PATH,0o755);chmodSync(env.MAEKLOG_CODEX_BIN,0o755);
 let panel;
 try{
  const hook=spawnSync(binary,['hook'],{env,input:JSON.stringify({hook_event_name:'SessionStart',session_id:'fixture',cwd:root}),encoding:'utf8',timeout:10000});
  assert.equal(hook.status,0,hook.stderr);assert.equal(hook.stderr,'');
  const space=join(state,'spaces',readdirSync(join(state,'spaces'))[0]);
  writeFileSync(join(root,'fixture-panel'),'fixture');
  let output='',committed='',committedStart=-1;
  const child=spawn('python3',[resolve('scripts/pty-model-picker.py'),binary],{env,stdio:['pipe','pipe','pipe']});
  child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
  child.stdout.on('data',s=>output+=s);child.stderr.on('data',s=>output+=s);child.stdin.on('error',()=>{});
  panel={child,columns:80,rows:40,send:keys=>child.stdin.write(keys),frame:()=>{
   const start=output.lastIndexOf('\x1b[2J'),candidate=output.slice(start),footer=candidate.lastIndexOf('\x1b['+panel.rows+';1H');
   const end=footer<0?-1:candidate.indexOf('\x1b[0m',footer);
   if(end>=0){committed=candidate;committedStart=start;}
   return committed;
  },text:()=>compact(panel.frame()),paint:()=>{panel.frame();return committedStart;}};
  const capture=name=>{const path=join(evidence,tag+'-'+name+'.ansi');writeFileSync(path,panel.frame());captures.push({name:tag+'-'+name,path,locale,columns:panel.columns,rows:panel.rows});};
  const repaint=async keys=>{const before=panel.paint();panel.send(keys);await until(()=>panel.paint()>before,'input did not finish painting');};
  const resize=async(signal,columns,rows)=>{const before=panel.paint();child.kill(signal);panel.columns=columns;panel.rows=rows;await until(()=>panel.paint()>before&&panel.frame().includes('\x1b['+rows+';1H'),'resize did not finish painting');};
  const configBytes=()=>readFileSync(settingsPath,'utf8');
  const tracked=['current.json','events.jsonl','pending.jsonl','events.pending.jsonl','participation.json','live-summary.json','manual-day.json','retry.json','refresh.json','rebuild.json','daily-policy.json','task-links.json'];
  const historyBytes=()=>Object.fromEntries(tracked.map(p=>[p,existsSync(join(space,p))?readFileSync(join(space,p),'utf8'):null]));
  const assertPure=(before,settings)=>{assert.equal(configBytes(),settings,'navigation changed model settings');assert.deepEqual(historyBytes(),before,'picker rewrote records or scheduled work');assert.equal(readFileSync(globalPath,'utf8'),'# Global Codex fixture: must remain byte-for-byte unchanged\nmodel = "global-fixture"\n');};
  let capturedMenu=false;
  const inPicker=()=>panel.text().includes(t('요약모델','Summarymodel'))&&panel.text().includes(t('현재추론강도:','Currentreasoning:'));
  const menuSelected=()=>panel.frame().match(/\x1b\[7m([^\x1b]*)/)?.[1]?.replace(/\s/g,'')??'';
  const open=async()=>{
   panel.send('m');await until(()=>panel.text().includes(t('기능선택','Chooseanaction')),'locale menu did not open');
   assert(menuSelected().startsWith(t('저장기록보기','Viewsavedrecords')),'first menu action is not read-only records');
   const frame=panel.frame(),footerAt=frame.lastIndexOf('\x1b['+panel.rows+';1H'),footerEnd=frame.indexOf('\x1b[0m',footerAt);
   const help=compact(frame.slice(footerAt,footerEnd));
   assert(help.includes('Enter')&&help.includes('Esc')&&!help.includes(t('r반영','rreflect'))&&!help.includes(t('m메뉴','mmenu')),'menu advertised inactive base controls');
   assert.equal((frame.match(/Enter/g)??[]).length,1,'menu displayed more than one active key help');
   assert(menuSelected().endsWith('h'),'saved-record shortcut is not displayed in its right-hand column');
   assert(panel.text().includes('언어/Language'),'bilingual language row missing');
   for(const label of [t('전체기록보기·h','Allrecords·h'),t('연결새로확인·R','Checklinksagain·R'),t('패널만숨기기·수집계속·q','Hidepanel·keepcollecting·q')])assert(!compact(frame).includes(label),'menu duplicated an old inline shortcut label');
   if(!capturedMenu){capture('menu-wide');capturedMenu=true;}
   await repaint('\x1b[B');
   assert(menuSelected().startsWith(t('요약모델','Summarymodel')),'model item unreachable');panel.send('\r');await until(inPicker,'model settings view did not open');
  };
  await until(()=>panel.text().includes(t('m메뉴','mmenu'))||panel.text().endsWith('·m'),'initial localized panel missing');
  const before=historyBytes(),initialBytes=configBytes();
  await open();await until(()=>panel.text().includes(t('확인중','LoadingCodex')),'loading state missing');capture('loading-wide');
  assert(panel.text().includes('fixture-unlisted')&&panel.text().includes('max'),'current configuration hidden during load');
  const beforeMove=panel.paint();panel.send('\x1b[B');await until(()=>panel.paint()>beforeMove,'loading input blocked the terminal',900);
  await until(()=>panel.text().includes('Fixture원문model')&&panel.text().includes(t('현재모델이목록에없습니다','Currentmodelisnotlisted')),'loaded/current-unlisted state missing');
  capture('models-wide');assertPure(before,initialBytes);
  await resize('SIGUSR1',30,28);capture('models-narrow');assert(panel.text().includes(t('r확인','rrefresh')),'narrow model refresh control hidden');
  await repaint('\x1b[F');assert(panel.text().includes('>Fixturelongmodel35'),'End did not reveal final model');capture('last-model-narrow');
  await resize('SIGALRM',30,8);capture('models-short');assert(panel.text().includes('>'),'short window hid selected model');
  await repaint('\x1b[H');await resize('SIGUSR2',80,40);
  await repaint('\r');await until(()=>panel.text().includes(t('추론강도선택','Choosereasoning')),'reasoning stage missing');
  assert(panel.text().includes('>high')&&panel.text().includes('max'),'reasoning options were not returned catalog options');
  await repaint('\x1b[B');capture('reasoning-wide');await resize('SIGUSR1',30,28);capture('reasoning-narrow');
  assertPure(before,initialBytes);panel.send('\x1b');await until(()=>!inPicker(),'Escape did not cancel reasoning');assertPure(before,initialBytes);
  assert.equal(child.exitCode,null,'Escape hid the panel');
  await resize('SIGUSR1',30,28);panel.send('m');await until(()=>panel.text().includes(t('기능선택','Chooseanaction')),'narrow menu missing');capture('menu-narrow');
  assert(panel.text().includes(t('요약모델','Summarymodel')),'model row hidden in narrow menu');
  await resize('SIGALRM',30,8);capture('menu-short');
  const shortFrame=panel.frame(),shortFooterAt=shortFrame.lastIndexOf('\x1b[8;1H');
  assert(compact(shortFrame.slice(shortFooterAt,shortFrame.indexOf('\x1b[0m',shortFooterAt))).includes('Esc'),'short menu overpainted active footer');
  panel.send('\x1b');await until(()=>!panel.text().includes(t('기능선택','Chooseanaction')),'short menu Escape failed');assertPure(before,initialBytes);
  // No-override models still require a separate confirmation, which Escape cancels.
  setMode('ok');await resize('SIGUSR2',80,40);await open();await until(()=>panel.text().includes('Fixture원문model'),'catalog did not reload');
  await repaint('\x1b[B');await repaint('\r');assert(panel.text().includes(t('추론강도지정안함','Noreasoningoverride')),'no-effort model invented an effort');
  capture('no-override-wide');panel.send('\x1b');await until(()=>!inPicker(),'no-override Escape failed');assertPure(before,initialBytes);
  // A no-effort model persists null only at the same final confirmation step.
  await open();await until(()=>panel.text().includes('Fixture원문model'),'no-override save catalog did not load');await repaint('\x1b[B');await repaint('\r');assertPure(before,initialBytes);
  panel.send('\r');await until(()=>!inPicker(),'no-override final Enter did not save');
  assert.deepEqual(JSON.parse(configBytes()),{model:'fixture-no-effort',effort:null});assert.deepEqual(historyBytes(),before,'no-override save rewrote history');
  const noOverrideBytes=configBytes();
  // Only a final Enter writes the Maeklog-specific shared setting.
  await open();await until(()=>panel.text().includes('Fixture원문model'),'save catalog did not load');await repaint('\x1b[H');await repaint('\r');await repaint('\x1b[B');
  assertPure(before,noOverrideBytes);panel.send('\r');await until(()=>!inPicker(),'final Enter did not save and return');
  assert.deepEqual(JSON.parse(configBytes()),{model:'fixture-choice',effort:'max'});
  assert.deepEqual(historyBytes(),before,'save rewrote records or queued inference');
  const other=spawnSync(binary,['model','show'],{env:{...env,HERDR_SESSION:'other-tab'},encoding:'utf8',timeout:5000});assert.equal(other.status,0,other.stderr);assert(other.stdout.includes('fixture-choice')&&other.stdout.includes('max'),'another tab did not see shared settings');
  const savedBytes=configBytes();
  // Failed refresh retains a clearly unverified prior list and cannot save it.
  await open();await until(()=>panel.text().includes('Fixture원문model'),'stale-test catalog did not load');setMode('fail');panel.send('r');
  await until(()=>panel.text().includes(t('이전목록은미확인','priorlistunverified')),'failed refresh was presented as fresh');capture('offline-wide');
  await repaint('\r');assert(!panel.text().includes(t('추론강도선택·Enter로저장','Choosereasoning·Entertosave')),'unverified list allowed confirmation');assertPure(before,savedBytes);
  await resize('SIGUSR1',30,28);capture('offline-narrow');
  setMode('empty');panel.send('r');await until(()=>panel.text().includes(t('반환한모델이없습니다','returnednomodels'))||panel.text().includes(t('표시가능한텍스트모델이없습니다','novisibletextmodels')),'empty catalog state missing');capture('empty-narrow');assert(!panel.text().includes(t('Enter선택','Enterchoose'))&&!panel.text().includes(t('Enter강도','Enterreasoning')),'empty view advertised unavailable confirmation');
  await repaint('\r');assertPure(before,savedBytes);
  setMode('ok',20000);panel.send('r');await until(()=>panel.text().includes(t('확인중','LoadingCodex')),'slow refresh missing');
  const opened=Date.now();panel.send('\x1b');await until(()=>!inPicker(),'cancel waited on discovery',1200);assert(Date.now()-opened<1200,'catalog blocked Escape');assertPure(before,savedBytes);
  // Bounded discovery processes must be reaped after cancel rather than leaking.
  await until(()=>{
   const records=existsSync(callsPath)?readFileSync(callsPath,'utf8').trim().split('\n').map(JSON.parse):[];
   return records.filter(v=>v.kind==='launch').every(v=>{try{process.kill(v.pid,0);return false;}catch{return true;}});
  },'cancelled model-list process remained alive',3000);
  const calls=readFileSync(callsPath,'utf8').trim().split('\n').map(JSON.parse);
  assert(calls.filter(c=>c.kind==='launch').every(c=>c.args[0]==='app-server'),'picker performed inference');
  assert(calls.filter(c=>c.kind==='rpc').every(c=>['initialize','initialized','config/read','model/list'].includes(c.method)),'picker ran a model turn');
  assert.equal(readFileSync(globalPath,'utf8'),'# Global Codex fixture: must remain byte-for-byte unchanged\nmodel = "global-fixture"\n');
  panel.send('q');await until(()=>child.exitCode!==null,'panel did not close');assert.equal(child.exitCode,0,output.slice(-3000));
  checks.push(tag+': system locale precedence; localized menu/default Records; loading input; dynamic models/efforts and source text; wide/narrow/short scroll; Escape pure; explicit shared save; no override; stale/empty/offline retry; cancelled child cleanup; zero inference/history/global config writes');
  console.log('PASS '+tag+': model picker and system locale PTY contracts');
 }finally{
  if(panel&&panel.child.exitCode===null){panel.send('\x1bq');await pause(300);if(panel.child.exitCode===null)panel.child.kill('SIGTERM');}
  rmSync(root,{recursive:true,force:true});
 }
}
result({passed:true});
