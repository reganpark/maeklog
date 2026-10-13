// Two real PTYs share an isolated Maeklog language preference. No real inference/network.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync,chmodSync} from 'node:fs';
import {join,resolve,dirname} from 'node:path';
import {tmpdir} from 'node:os';
import {spawn,spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const evidence=resolve(process.argv[3]??mkdtempSync(join(tmpdir(),'maeklog-language-evidence-')));
mkdirSync(evidence,{recursive:true});
const root=mkdtempSync(join(tmpdir(),'maeklog-language-')),config=join(root,'config'),home=join(root,'home'),repo=join(root,'repo');
for(const p of [config,home,repo,join(home,'.codex')])mkdirSync(p,{recursive:true});
const settings=join(config,'language-settings.json'),modelSettings=join(config,'model-settings.json'),global=join(home,'.codex','config.toml'),calls=join(root,'unexpected-calls.jsonl');
writeFileSync(modelSettings,JSON.stringify({model:'fixture-current-model',effort:'max'}));
writeFileSync(global,'# Untouched global Codex configuration\nmodel = "global-fixture"\n');
const protectedConfig={[modelSettings]:readFileSync(modelSettings,'utf8'),[global]:readFileSync(global,'utf8')};
const digest=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const candidateSha=digest(binary),scriptSha=digest(new URL(import.meta.url));
const compact=s=>s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
const strip=s=>s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'');
const pause=ms=>new Promise(done=>setTimeout(done,ms));
async function until(check,message,timeout=10000){const end=Date.now()+timeout;while(Date.now()<end){if(check())return;await pause(25);}throw Error(typeof message==='function'?message():message);}
const panels=[],checks=[],captures=[];
const result=extra=>writeFileSync(join(evidence,'result.json'),JSON.stringify({candidate_sha256:candidateSha,script_sha256:scriptSha,checks,captures,...extra},null,2));
const pref=()=>existsSync(settings)?JSON.parse(readFileSync(settings,'utf8')).language:'auto';
const bytes=p=>existsSync(p)?readFileSync(p,'utf8'):null;
const rawQuote='원문 한국어 / English / Auto · café 🐱 · quoted unchanged';
const originalRecord='보존할 완료 보고 — Exact original statement';
const newRecord='새로 저장된 보고 — Must not replace a frozen reader';
const isLanguage=p=>p.text().startsWith('언어/Language')&&(p.text().includes('Saved:')||p.text().includes('저장됨:'));
const isReader=p=>p.text().includes('Frozencontent')||p.text().includes('고정된내용');
const mainLanguage=(p,ko)=>p.text().includes(ko?'현재대화의작업':'Currentconversationtask')&&!isLanguage(p)&&!isReader(p);
const menuSelected=p=>p.frame().match(/\x1b\[7m([^\x1b]*)/)?.[1]?.replace(/\s/g,'')??'';
const choiceSelected=(p,name)=>p.text().includes('>'+compact(name));
const capture=(p,name)=>{const path=join(evidence,name+'.ansi');writeFileSync(path,p.frame());captures.push({name,path,columns:p.columns,rows:p.rows});};
function assertPure(protectedFiles){
 for(const [path,value] of Object.entries({...protectedConfig,...protectedFiles}))assert.equal(bytes(path),value,'language navigation/save changed '+path);
 assert(!existsSync(calls),'language or read-only navigation invoked Codex/GitHub/opener');
}
async function openLanguage(p){
 p.send('m');await until(()=>p.text().includes('기능선택')||p.text().includes('Chooseanaction'),'menu missing');
 assert(menuSelected(p).startsWith('저장기록보기')||menuSelected(p).startsWith('Viewsavedrecords'),'Records is not the safe default');
 assert(menuSelected(p).endsWith('h'),'right-hand Records key missing');
 await p.repaint('\x1b[B');assert(menuSelected(p).startsWith('요약모델')||menuSelected(p).startsWith('Summarymodel'),'Summary model is not second');
 await p.repaint('\x1b[B');assert(menuSelected(p).startsWith('언어/Language'),'bilingual language entry is not third');
 p.send('\r');await until(()=>isLanguage(p),'language picker missing');
}
async function choose(p,language){
 await openLanguage(p);
 if(pref()!=='auto')capture(p,'saved-'+pref()+'-picker-wide');
 await p.repaint('\x1b[H');
 if(language==='en')await p.repaint('\x1b[B');
 if(language==='ko')await p.repaint('\x1b[F');
 assert(choiceSelected(p,{auto:'자동 / Auto',en:'English',ko:'한국어'}[language]),'chosen language not painted');
 p.send('\r');await until(()=>!isLanguage(p)&&pref()===language,'explicit language save did not close/commit');
}
function optionRow(p,label){
 for(const m of p.frame().matchAll(/\x1b\[(\d+);1H([\s\S]*?)(?=\x1b\[\d+;1H|$)/g)){
  const text=strip(m[2]).trim();if(text.startsWith('> '+label)||text.startsWith(label))return Number(m[1]);
 }
 throw Error('rendered option row not found: '+label);
}
async function createPanel(tag,locale){
 const state=join(root,tag);mkdirSync(state);
 const env={...process.env,LC_ALL:locale,LANG:locale,HOME:home,CODEX_HOME:join(home,'.codex'),XDG_CONFIG_HOME:join(root,'xdg'),HERDR_PLUGIN_CONFIG_DIR:config,
 HERDR_WORKSPACE_ID:'fixture-'+tag,HERDR_TAB_ID:'fixture:'+tag,HERDR_PANE_ID:'fixture-'+tag+':pm',HERDR_SESSION:'language-'+tag,
 MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),PM_TEST_ROOT:root,PM_TEST_BINARY:binary,
 MAEKLOG_CODEX_BIN:resolve('scripts/mock-language-picker-guard.mjs'),MAEKLOG_GH_BIN:resolve('scripts/mock-language-picker-guard.mjs'),MAEKLOG_OPEN_BIN:resolve('scripts/mock-language-picker-guard.mjs'),
 MAEKLOG_LANGUAGE_TEST_CALLS:calls,MAEKLOG_DISABLE_LUNA:'1',TERM:'xterm-256color',COLORTERM:'truecolor',TZ:'Asia/Seoul'};
 delete env.MAEKLOG_INTERNAL;delete env.NO_COLOR;
 const run=(args,input)=>spawnSync(binary,args,{env,input:input===undefined?undefined:JSON.stringify(input),encoding:'utf8',timeout:10000});
 const hook=run(['hook'],{hook_event_name:'UserPromptSubmit',session_id:'thread-'+tag,turn_id:'turn-'+tag,cwd:repo,prompt:rawQuote});
 assert.equal(hook.status,0,hook.stderr);
 const space=join(state,'spaces',readdirSync(join(state,'spaces'))[0]);
 const source=Object.values(JSON.parse(readFileSync(join(space,'event-sources.json'),'utf8'))).find(source=>source.kind==='user_prompt');
 const journal=join(state,'branches',source.context.key,'journal.json');mkdirSync(dirname(journal),{recursive:true});
 const at=Math.floor(Date.now()/1000),proof=[{source,quote:rawQuote}];
 const saved={revision:1,through_order:1,through_unix_utc:at,updated_at_unix_utc:at,records:{C1:{id:'C1',key:'C1',kind:'completion',stage:'deployment',outcome:'reported',
 versions:[{operation:'add',text:originalRecord,condition:null,at_unix_utc:at,order:1,target:null,proof}],replaced_by:null,withdrawn:false,follows:null,relation_order:0}},states:{},gaps:[]};
 writeFileSync(journal,JSON.stringify(saved));
 writeFileSync(join(root,'fixture-'+tag+'-panel'),'fixture');
 let output='',committed='',committedStart=-1;
 const child=spawn('python3',[resolve('scripts/pty-language-picker.py'),binary],{env,stdio:['pipe','pipe','pipe']});
 child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
 child.stdout.on('data',s=>output+=s);child.stderr.on('data',s=>output+=s);child.stdin.on('error',()=>{});
 const p={tag,child,space,journal,saved,rows:40,columns:80,send:keys=>child.stdin.write(keys),output:()=>output,run,
 frame:()=>{const start=output.lastIndexOf('\x1b[2J'),candidate=output.slice(start),footer=candidate.lastIndexOf('\x1b['+p.rows+';1H');
  if(footer>=0&&candidate.indexOf('\x1b[0m',footer)>=0){committed=candidate;committedStart=start;}return committed;},
 text:()=>compact(p.frame()),paint:()=>{p.frame();return committedStart;},
 repaint:async keys=>{const before=p.paint();p.send(keys);await until(()=>p.paint()>before,'input did not complete a frame');},
 resize:async(signal,columns,rows)=>{const before=p.paint();p.columns=columns;p.rows=rows;child.kill(signal);await until(()=>p.paint()>before,'resize did not complete a frame');}};
 panels.push(p);await until(()=>mainLanguage(p,locale.startsWith('ko')),'Auto did not follow panel locale');return p;
}
result({passed:false});
try{
 for(const script of ['scripts/mock-herdr.mjs','scripts/mock-language-picker-guard.mjs'])chmodSync(script,0o755);
 for(const args of [['init','-q','-b','main'],['-c','user.name=Fixture','-c','user.email=fixture@invalid','commit','--allow-empty','-m','fixture']]){
  const git=spawnSync('git',args,{cwd:repo,env:{...process.env,HOME:home,XDG_CONFIG_HOME:join(root,'xdg')},encoding:'utf8'});assert.equal(git.status,0,git.stderr);
 }
 const ko=await createPanel('ko','ko_KR.UTF-8'),en=await createPanel('en','en_US.UTF-8');
 const protectedFiles=()=>Object.fromEntries(panels.flatMap(p=>[p.journal,...['current.json','task-links.json','inbox.jsonl','live-summary.json','manual-day.json','retry.json','refresh.json','rebuild.json','participation.json'].map(name=>join(p.space,name))]).map(path=>[path,bytes(path)]));
 let before=protectedFiles();
 ko.send('m');await until(()=>ko.text().includes('기능선택'),'Korean menu missing');capture(ko,'ko-menu-wide');ko.send('\x1b');await until(()=>mainLanguage(ko,true),'menu cancel missing');
 en.send('m');await until(()=>en.text().includes('Chooseanaction'),'English menu missing');capture(en,'en-menu-wide');en.send('\x1b');await until(()=>mainLanguage(en,false),'English menu cancel missing');
 await openLanguage(en);capture(en,'en-auto-wide');await en.resize('SIGUSR1',30,28);capture(en,'en-auto-narrow');await en.resize('SIGALRM',30,8);capture(en,'en-auto-short');await en.resize('SIGUSR2',80,40);en.send('\x1b');await until(()=>mainLanguage(en,false),'English picker cancel missing');
 await openLanguage(ko);capture(ko,'ko-auto-wide');
 assert(ko.text().includes('자동적용:한국어'),'Auto effective language missing');
 await ko.repaint('\x1b[B');assert(choiceSelected(ko,'English'));assert.equal(bytes(settings),null,'arrows saved a preference');
 ko.send('msvrRhlbeoq\x1b[21~\x1b[<2;4;4M\x1b[<2;4;4m');await pause(300);
 assert(isLanguage(ko)&&choiceSelected(ko,'English'),'language view leaked shortcut input');assertPure(before);
 await ko.resize('SIGUSR1',30,28);capture(ko,'ko-pending-narrow');await ko.resize('SIGALRM',30,8);capture(ko,'ko-pending-short');
 assert(ko.frame().includes('\x1b[8;1H')&&ko.text().includes('Enter'),'short window lost active controls');
 await ko.resize('SIGUSR2',80,40);ko.send('\x1b');await until(()=>mainLanguage(ko,true),'cancel changed panel language');assert.equal(bytes(settings),null);assertPure(before);
 checks.push('Auto uses each process locale; model second/language third; stable self names; arrow-only highlight, modal shortcut isolation, wide/narrow/short frame and pure Escape');

 // Restored menu keys open the same safe views; only explicit s/r request intent.
 for(const [key,visible] of [['h',()=>isReader(ko)],['l',()=>ko.text().includes('작업연결정정')],['e',()=>ko.text().includes('근거보기·사용자정정')],['o',()=>ko.text().includes('작업연결정정')],['b',()=>ko.text().includes('날짜를선택하세요')]]){
  ko.send('m');await until(()=>ko.text().includes('기능선택'),'shortcut menu missing');ko.send(key);await until(visible,'menu '+key+' did not route to its view');
  ko.send('\x1b');await until(()=>mainLanguage(ko,true),'menu '+key+' view did not cancel safely');
 }
 assertPure(before);
 for(const key of ['s','r','v']){ko.send('m');await until(()=>ko.text().includes('기능선택'),'request menu missing');ko.send(key);await until(()=>!ko.text().includes('기능선택'),'explicit request shortcut did not activate');}
 await until(()=>{const path=join(ko.space,'live-summary.json');return !existsSync(path)||!['queued','running'].includes(JSON.parse(readFileSync(path,'utf8')).status);},'explicit disabled summary request did not settle');
 assert(!existsSync(calls),'explicit disabled/local request made external calls');before=protectedFiles();
 checks.push('advertised h/l/e/o/b shortcuts route to their safe views; s/r/v activate only explicit menu intent; zero external calls');

 // Keep the other pane in a frozen evidence category while the preference changes.
 en.send('h');await until(()=>isReader(en),'second-panel reader missing');
 const category=()=>en.text().match(/(\d+)\/(\d+)·/)?.slice(1).map(Number);
 const total=category()?.[1];assert(total>1);
 for(let n=0;n<total&&!en.text().includes('Changesandevidence');n++){const previous=category()[0];en.send(']');await until(()=>category()?.[0]!==previous,'reader category did not change');}
 await until(()=>en.text().includes('Changesandevidence')&&en.text().includes(compact(rawQuote)),'saved source evidence not reachable');
 const originalCategory=category()[0],captured=en.text().match(/Captured([^·]+)·Frozencontent/)?.[1];assert(captured);
 const changed=structuredClone(en.saved);changed.revision=2;changed.records.C1.versions[0].text=newRecord;writeFileSync(en.journal,JSON.stringify(changed));before=protectedFiles();
 const copyBefore=en.output().split('\x1b]52;').length;
 en.send('\x1b[<0;1;1M\x1b[<32;4;1M');await until(()=>en.text().includes('Selecting'),'reader selection missing');
 await choose(ko,'ko');await until(()=>en.text().includes('전체기록·읽기전용')&&en.text().includes('선택해제'),'other-panel reader did not refresh/clear visual selection');
 assert.equal(category()[0],originalCategory);assert(en.text().includes('열람시점'+captured+'·고정된내용'));
 assert(en.text().includes(compact(originalRecord))&&en.text().includes(compact(rawQuote))&&!en.text().includes(compact(newRecord)),'language reload replaced frozen source evidence');
 en.send('\x1b[<0;4;1m');await pause(250);assert.equal(en.output().split('\x1b]52;').length,copyBefore,'cleared selection copied stale coordinates');capture(en,'ko-frozen-evidence');assertPure(before);
 await choose(ko,'en');await until(()=>en.text().includes('Allrecords·readonly')&&en.text().includes('Changesandevidence'),'other-panel reader did not switch back to English');
 assert.equal(category()[0],originalCategory);assert(en.text().includes('Captured'+captured+'·Frozencontent'));assert(en.text().includes(compact(rawQuote)));capture(en,'en-frozen-evidence');assertPure(before);
 en.send('\x1b');await until(()=>mainLanguage(en,false)&&en.text().includes(compact(newRecord)),'reader return did not expose newer live stored data');
 checks.push('shared immediate En/Ko reload; frozen reader retains original record/quote/capture/category despite newer journal; selection clears without copying wrong text; underlying files unchanged');

 // A saved mode change may resolve to the same language in the other panel.
 await openLanguage(en);await en.repaint('\x1b[F');assert(choiceSelected(en,'한국어'));
 await choose(ko,'auto');await until(()=>en.text().includes('Saved:자동/Auto')&&en.text().includes('Autoresolvesto:English'),'same-effective Auto mode did not refresh the open picker saved marker');
 assert(choiceSelected(en,'한국어'),'shared reload overwrote an unsaved highlight');capture(en,'en-auto-shared-pending');
 en.send('\x1b');await until(()=>mainLanguage(en,false),'shared pending cancel missing');assert.equal(pref(),'auto');await until(()=>mainLanguage(ko,true),'Auto did not restore Korean locale');assertPure(before);

 await openLanguage(ko);const englishRow=optionRow(ko,'English'),koreanRow=optionRow(ko,'한국어'),prior=bytes(settings);
 ko.send(`\x1b[<0;3;${englishRow}M\x1b[<0;3;${koreanRow}m`);await pause(250);assert(isLanguage(ko));assert.equal(bytes(settings),prior,'mismatched click saved a preference');
 ko.send(`\x1b[<0;3;${englishRow}M\x1b[<0;3;${englishRow}m`);await until(()=>pref()==='en'&&mainLanguage(ko,false),'completed option click did not save/apply');await until(()=>mainLanguage(en,false),'clicked shared mode not applied');
 await openLanguage(ko);const clickBefore=bytes(settings);ko.send('\x1b[<0;2;1M\x1b[<0;2;1m');await until(()=>!isLanguage(ko),'outside click did not cancel');assert.equal(bytes(settings),clickBefore);assertPure(before);
 checks.push('same-effective shared mode reload preserves pending highlight; completed option click applies once; mismatched/outside click cancel without writes');

 const malformed='{"language":"unsupported"}';writeFileSync(settings,malformed);await openLanguage(ko);await ko.repaint('\x1b[F');ko.send('\r');
 await until(()=>isLanguage(ko)&&ko.text().includes('Couldnotread/savesettings'),'failed language save did not remain visible');
 assert.equal(bytes(settings),malformed);assert(ko.text().includes('CurrentUI:English'),'failed save changed effective UI language');capture(ko,'en-save-error');assertPure(before);
 ko.send('\x1b');await until(()=>mainLanguage(ko,false),'failed preference view did not cancel');
 writeFileSync(settings,JSON.stringify({language:'auto'}));await until(()=>mainLanguage(ko,true)&&mainLanguage(en,false),'valid Auto restoration not observed');
 checks.push('malformed preferences retain last effective language and original file; save error stays in view; default Auto restored; global Codex/model settings untouched');

 // Advertised q hides only the selected pane; Esc remains cancellation.
 ko.send('m');await until(()=>ko.text().includes('기능선택'),'hide menu missing');ko.send('q');await until(()=>ko.child.exitCode!==null,'advertised q did not hide its panel');assert.equal(ko.child.exitCode,0);
 assert.equal(JSON.parse(readFileSync(join(ko.space,'ui.json'),'utf8')).visible,false);assert.equal(JSON.parse(readFileSync(join(en.space,'ui.json'),'utf8')).visible,true);assert.equal(bytes(join(ko.space,'participation.json')),before[join(ko.space,'participation.json')]);
 en.send('q');await until(()=>en.child.exitCode!==null,'second panel cleanup failed');assert.equal(en.child.exitCode,0);
 assert.equal(digest(binary),candidateSha);assert.equal(digest(new URL(import.meta.url)),scriptSha);result({passed:true});
 console.log('PASS language picker: '+checks.join('; '));
}catch(error){for(const p of panels)capture(p,'failure-'+p.tag);result({passed:false,error:String(error.stack??error),saved_preference:bytes(settings),panels:panels.map(p=>({tag:p.tag,exit_code:p.child.exitCode,text:p.text()}))});throw error;}
finally{
 for(const p of panels)if(p.child.exitCode===null){p.send('\x1b\x1bq');await pause(300);if(p.child.exitCode===null)p.child.kill('SIGTERM');}
 rmSync(root,{recursive:true,force:true});
}
