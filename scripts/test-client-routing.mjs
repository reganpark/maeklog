// Real bridge command -> Herdr fixture -> hook -> scoped event stores, no model.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,readdirSync,existsSync,rmSync,chmodSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const root=mkdtempSync(join(tmpdir(),'pm-client-routing-')),state=join(root,'state'),topology=join(root,'topology.json');
const json=p=>JSON.parse(readFileSync(p,'utf8')),put=(p,v)=>writeFileSync(p,JSON.stringify(v));
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:state,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_ENV:'1',HERDR_WORKSPACE_ID:'wA',HERDR_TAB_ID:'wA:t1',HERDR_PANE_ID:'wA:p1',HERDR_SESSION:'routing-test',HERDR_SOCKET_PATH:'/fixture/server.sock',HERDR_BIN_PATH:resolve('scripts/mock-client-herdr.mjs'),PM_CLIENT_TOPOLOGY:topology,MAEKLOG_DISABLE_LUNA:'1'};
const spools=new Set();
delete env.MAEKLOG_INTERNAL;
chmodSync(env.HERDR_BIN_PATH,0o755);mkdirSync(state);
const pane=(n)=>({pane_id:`wA:p${n}`,workspace_id:'wA',tab_id:`wA:t${n}`,terminal_id:`terminal-${n}`,pid:100+n,agent:'codex',cwd:root});
put(topology,{socket:env.HERDR_SOCKET_PATH,panes:[pane(1),pane(2)]});
const run=(args,input,extra={})=>spawnSync(binary,args,{env:{...env,...extra},input:input?JSON.stringify(input):undefined,encoding:'utf8',timeout:10000});
const client=(action,n,thread)=>{const r=run(['client',action,String(100+n),thread],null,{HERDR_PANE_ID:`wA:p${n}`});assert.equal(r.status,0,r.stderr);};
const hook=(session,turn,event='UserPromptSubmit',good=true)=>{
 const r=run(['hook'],{session_id:session,turn_id:turn,hook_event_name:event,cwd:root,prompt:session+' request',last_assistant_message:session+' result'}, {HERDR_SOCKET_PATH:'/wrong/daemon.sock',HERDR_BIN_PATH:'/wrong/daemon/herdr'});
 assert.equal(r.status,0,r.stderr);
 const output=JSON.parse(r.stdout);
 if(good&&event==='UserPromptSubmit'&&output.hookSpecificOutput){
  assert.equal(output.hookSpecificOutput.hookEventName,'UserPromptSubmit');
  assert.match(output.hookSpecificOutput.additionalContext,/MAEKLOG_TASK_REPORT_CONTEXT_V1 /);
  spools.add(JSON.parse(output.hookSpecificOutput.additionalContext.split('\n')[0].slice('MAEKLOG_TASK_REPORT_CONTEXT_V1 '.length)).spool);
 }else assert.deepEqual(output,{});
 if(good)assert.equal(r.stderr,'');else assert.match(r.stderr,/maeklog hook:/);
};
const stores=()=>existsSync(join(state,'spaces'))?readdirSync(join(state,'spaces')).map(k=>({dir:join(state,'spaces',k),meta:json(join(state,'spaces',k,'meta.json'))})):[];
const store=tab=>stores().find(s=>s.meta.tab_id===tab).dir;
const events=tab=>{const p=join(store(tab),'inbox.jsonl');return existsSync(p)?readFileSync(p,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];};
const guard=spawn('python3',['-c','import fcntl,sys; f=open(sys.argv[1],"a+"); fcntl.flock(f,fcntl.LOCK_EX); print("ready",flush=True); sys.stdin.read()',join(state,'worker.lock')],{stdio:['pipe','pipe','inherit']});
await new Promise((resolve,reject)=>{guard.stdout.once('data',resolve);guard.once('error',reject);});
try {
 // Enable strict routing before the first SessionStart can race registration.
 put(join(state,'client-routing.json'),{clients:[],turns:{}});
 hook('early','turn-early','SessionStart');assert.equal(stores().length,0);
 hook('unknown','unregistered','UserPromptSubmit',false);assert.equal(stores().length,0);
 // A fresh TUI may not yet have a screen-derived agent label. Its exact
 // foreground Codex PID still proves ownership and permits initial reporting.
 const cold=json(topology);delete cold.panes[0].agent;put(topology,cold);
 client('attach',1,'thread-a');client('attach',2,'thread-b');
 // The matching PID alone is insufficient if the foreground program is a shell.
 const shell=json(topology);shell.panes[0].process_name='bash';put(topology,shell);
 const rejected=run(['client','attach','101','not-codex']);
 assert.notEqual(rejected.status,0);assert.match(rejected.stderr,/no longer owns the terminal/);
 assert(!json(join(state,'client-routing.json')).clients.some(c=>c.thread==='not-codex'));
 delete shell.panes[0].process_name;put(topology,shell);
 const outside=run(['hook'],{hook_event_name:'SessionStart',session_id:'thread-b',cwd:root},{HERDR_WORKSPACE_ID:undefined,HERDR_TAB_ID:undefined,HERDR_PANE_ID:undefined,HERDR_SOCKET_PATH:undefined});
 assert.equal(outside.stderr,'');assert.equal(outside.stdout.trim(),'{}');
 assert.equal(json(join(store('wA:t2'),'connection.json')).session_id,'thread-b');
 assert.equal(json(topology).panes[1].agent_session.value,'thread-b');
 hook('thread-a','a1');hook('thread-b','b1');
 assert.deepEqual(events('wA:t1').map(e=>e.session_id),['thread-a']);
 assert.deepEqual(events('wA:t2').map(e=>e.session_id),['thread-b']);
 const stale=json(topology);stale.panes[1].agent_session.value='wrong-daemon-report';put(topology,stale);
 hook('thread-b','bad-native-state','UserPromptSubmit',false);
 assert.equal(events('wA:t2').length,1);
 client('attach',2,'thread-b'); // refresh before submission repairs native registration
 // Switching the visible thread preserves the origin of an in-flight old turn.
 client('attach',2,'thread-c');client('detach',2,'thread-b');
 assert.equal(json(topology).panes[1].agent_session.value,'thread-c');
 hook('thread-b','b1','Stop');hook('thread-c','c1');
 assert.deepEqual(events('wA:t2').map(e=>e.kind),['user_prompt','assistant_final','user_prompt']);
 hook('thread-c','no-prompt','Stop',false);
 // Same conversation in two terminals is ambiguous; never choose the focused one.
 client('attach',1,'thread-c');
 const count=events('wA:t1').length+events('wA:t2').length;
 hook('thread-c','ambiguous','UserPromptSubmit',false);
 assert.equal(events('wA:t1').length+events('wA:t2').length,count);
 hook('thread-c','c1','Stop'); // already bound before the second viewer opened
 client('attach',1,'thread-a');
 // A moved terminal keeps its identity while new turns get its new tab scope.
 const moved=json(topology);Object.assign(moved.panes[1],{pane_id:'wB:p7',workspace_id:'wB',tab_id:'wB:t4'});moved.aliases={'wA:p2':'wB:p7'};put(topology,moved);
 client('attach',2,'thread-c');hook('thread-c','c2');
 assert.equal(events('wB:t4')[0].session_id,'thread-c');
 // Disabled scope stays disabled, without changing another tab's collection.
 put(join(store('wB:t4'),'participation.json'),{enabled:false,generation:1,history_blocked:true,open_turns:{}});
 hook('thread-c','off');assert.equal(events('wB:t4').length,1);
 hook('thread-a','a2');assert.equal(events('wA:t1').length,2);
 // Crash/reused terminal: the old client PID cannot route events into a new process.
 const replaced=json(topology);replaced.panes[0].pid=999;put(topology,replaced);
 hook('thread-a','stale','UserPromptSubmit',false);hook('thread-a','a1','Stop',false);
 assert.equal(events('wA:t1').length,2);
 client('detach',2,'thread-c');assert(!json(topology).panes[1].agent_session);
 hook('thread-c','closed','UserPromptSubmit',false);
 assert(!readFileSync(join(state,'client-routing.json'),'utf8').includes(' request'));
 console.log('PASS client routing: delayed screen detection, non-Codex PID rejection, daemon env ignored, same-cwd isolation, native registration, switch/late final, stale detach, ambiguous viewers, moved pane, opt-out, replaced client, disconnect, no model');
} finally {guard.stdin.end();await new Promise(resolve=>guard.once('exit',resolve));for(const spool of spools)rmSync(join(spool,'..'),{recursive:true,force:true});rmSync(root,{recursive:true,force:true});}
