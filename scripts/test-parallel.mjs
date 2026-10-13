// Real processes and file locks, mock Luna only: no AI calls or external services.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,existsSync,rmSync,unlinkSync,appendFileSync} from 'node:fs';
import {spawn,spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const root=mkdtempSync(join(tmpdir(),'pm-parallel-'));
const gate=join(root,'gates');mkdirSync(gate);
const traceFile=join(root,'trace.jsonl');
const env={...process.env,LC_ALL:'ko_KR.UTF-8',MAEKLOG_STATE_DIR:root,MAEKLOG_PLUGIN_ROOT:process.cwd(),
  MAEKLOG_CODEX_BIN:resolve('scripts/mock-reducer.mjs'),PM_TEST_CALLS:join(root,'calls.jsonl'),
  PM_TEST_TRACE:traceFile,PM_TEST_GATE_DIR:gate,PM_TEST_FAIL:'0',PM_TEST_FAIL_SPACES:'wfail'};
delete env.MAEKLOG_DISABLE_LUNA;
const put=(space,file,value)=>writeFileSync(join(root,'spaces',space,file),JSON.stringify(value));
const read=(space,file)=>JSON.parse(readFileSync(join(root,'spaces',space,file),'utf8'));
const events=(space,offset=0)=>[1,2].map(seq=>({seq:seq+offset,kind:seq===1?'user_prompt':'assistant_final',
  session_id:space,content:'test',truncated:false,recorded_at_unix:Math.floor(Date.now()/1000),cwd:join(root,space)}));
const traces=()=>existsSync(traceFile)?readFileSync(traceFile,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(check,message,ms=10000){const end=Date.now()+ms;while(Date.now()<end){if(check())return;await pause(25);}throw new Error(message);}
const coordinators=[];
const start=()=>{
  const child=spawn(binary,['worker'],{env,stdio:['ignore','ignore','pipe']});
  child.on('error',()=>{});child.stderr.resume();coordinators.push(child);return child;
};
try {
  for(const space of ['w0','w1','w2','w3','w4','wfail','wretry']) {
    mkdirSync(join(root,'spaces',space),{recursive:true,mode:0o700});
    put(space,'meta.json',{space_key:space,workspace_id:space,session_name:'test',cwd:join(root,space),label:space,created_at_unix:1,last_seen_at_unix:1,archived:false});
    put(space,'current.json',{revision:1,base_revision:0,source_seq:0,updated_at_unix:1,summary:{objective:'테스트 프로젝트 목표'}});
    writeFileSync(join(root,'spaces',space,'inbox.jsonl'),events(space).map(JSON.stringify).join('\n')+'\n');
    writeFileSync(join(gate,space),'hold');
  }
  put('wretry','retry.json',{key:'daily:today:2',attempts:1,next_at:Math.floor(Date.now()/1000)+120});
  put('wretry','latest-retry.json',{key:'latest:2',attempts:3,next_at:0});
  put('wretry','status.json',{state:'retry_wait',message:'test retry waiting'});
  put('wfail','retry.json',{key:'daily:today:2',attempts:2,next_at:0});
  put('wfail','latest-retry.json',{key:'latest:2',attempts:3,next_at:0});
  // Corrupt metadata in one space must not bring down healthy peers.
  mkdirSync(join(root,'spaces','wbad'));writeFileSync(join(root,'spaces','wbad','meta.json'),'{broken');
  const original=start();
  await until(()=>traces().filter(t=>t.type==='start').length===3,'three spaces did not start together');
  let starts=traces().filter(t=>t.type==='start');
  assert.deepEqual(starts.map(s=>s.space).sort(),['w0','w1','w2']);
  assert.equal(new Set(starts.map(s=>s.worker)).size,3);
  // Same-space direct invocation and fourth-space invocation cannot bypass the leases.
  for(const key of ['w0','w3']) assert.equal(spawnSync(binary,['worker-space',key],{env,timeout:3000}).status,0);
  assert.equal(traces().filter(t=>t.type==='start').length,3);
  assert.notEqual(spawnSync(binary,['worker-space','../w0'],{env,timeout:3000}).status,0);
  // Restart only the coordinator while its three independently running attempts survive.
  original.kill('SIGKILL');await until(()=>original.signalCode==='SIGKILL','old coordinator did not exit');
  const resumed=start();
  await pause(500);
  assert.equal(traces().filter(t=>t.type==='start').length,3,'restart duplicated in-flight work');
  // A slow w0 must not stop a freed slot from serving w3/w4.
  appendFileSync(join(root,'spaces','w1','inbox.jsonl'),events('w1',2).map(JSON.stringify).join('\n')+'\n');
  unlinkSync(join(gate,'w1'));
  await until(()=>traces().some(t=>t.type==='start' && ['w3','w4','wfail'].includes(t.space)), 'free slot waited for slow w0');
  assert.equal(traces().some(t=>t.type==='end'&&t.space==='w0'),false);
  assert.equal(traces().some(t=>t.type==='start'&&t.space==='wretry'),false);
  // Finish the mock calls; keep the retry budget exhausted so no delayed test call escapes.
  put('wretry','retry.json',{key:'daily:today:2',attempts:3,next_at:0});
  for(const key of ['w0','w2','w3','w4','wfail']) unlinkSync(join(gate,key));
  await until(()=>resumed.exitCode!==null,'coordinator did not finish',15000);
  assert.equal(resumed.exitCode,0);
  const active=new Set();let peak=0;
  for(const record of traces()) {
    if(record.type==='start') {
      assert(!active.has(record.space),'same-space overlap');active.add(record.space);
      peak=Math.max(peak,active.size);assert(active.size<=3,'more than three model calls');
    } else {assert(active.delete(record.space),'unmatched end');}
  }
  assert.equal(peak,3);assert.equal(active.size,0);
  for(const key of ['w0','w1','w2','w3','w4']) {
    assert.equal(read(key,'current.json').source_seq,key==='w1'?4:2);
    assert.equal(readFileSync(join(root,'spaces',key,'inbox.jsonl'),'utf8').trim(),'');
  }
  assert.equal(read('wretry','current.json').source_seq,0);
  assert.equal(read('wfail','current.json').source_seq,0);
  assert.equal(read('wfail','status.json').state,'error');
  assert.equal(read('wfail','retry.json').attempts,3);
  assert.equal(traces().filter(t=>t.type==='start').length,7);
  console.log('PASS: three concurrent spaces, one attempt per space, no fourth slot, slow/retry/fault isolation, coordinator SIGKILL/restart, no duplicates, concurrent incoming records preserved');
} finally {
  // Only fixture-owned processes: release test gates before stopping coordinators.
  rmSync(gate,{recursive:true,force:true});
  for(const child of coordinators) if(child.exitCode===null&&child.signalCode===null) child.kill('SIGTERM');
  await pause(500);
  // Give detached fixture workers time to finish before removing their state.
  await until(()=>{
    const live=new Set();for(const t of traces()) t.type==='start'?live.add(t.pid):live.delete(t.pid);
    return live.size===0;
  },'fixture models did not exit',5000);
  rmSync(root,{recursive:true,force:true});
}
