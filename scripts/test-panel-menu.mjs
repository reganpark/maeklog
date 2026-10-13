// Pseudo-terminal + real crossterm SGR mouse events. Mock Herdr only; no live Space changes.
import assert from 'node:assert/strict';
import {mkdtempSync,writeFileSync,readFileSync,readdirSync,existsSync,rmSync,chmodSync} from 'node:fs';
import {spawn,spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
const binary=resolve(process.argv[2]??'target/debug/maeklog');
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(check,message){const end=Date.now()+10000;while(Date.now()<end){if(check())return;await pause(20);}throw new Error(typeof message==='function'?message():message);}
for(const action of ['records','cancel','hide','disable']) {
  const root=mkdtempSync(join(tmpdir(),'pm-menu-'));
  const env={...process.env,LC_ALL:'ko_KR.UTF-8',HERDR_WORKSPACE_ID:'fixture',HERDR_PANE_ID:'fixture:pm',HERDR_SESSION:'menu-test',
    MAEKLOG_STATE_DIR:root,MAEKLOG_PLUGIN_ROOT:process.cwd(),HERDR_BIN_PATH:resolve('scripts/mock-herdr.mjs'),
    PM_TEST_ROOT:root,PM_TEST_BINARY:binary,MAEKLOG_DISABLE_LUNA:'1',TERM:'xterm-256color'};
  delete env.MAEKLOG_INTERNAL;
  chmodSync(env.HERDR_BIN_PATH,0o755);
  let child;
  try {
    const hook=spawnSync(binary,['hook'],{env,input:JSON.stringify({hook_event_name:'SessionStart',session_id:'fixture',cwd:root}),encoding:'utf8'});
    assert.equal(hook.status,0);assert.equal(hook.stderr,'');
    const space=join(root,'spaces',readdirSync(join(root,'spaces'))[0]);
    writeFileSync(join(space,'current.json'),JSON.stringify({revision:1,source_seq:0,summary:{objective:'메뉴 입력 검증'}}));
    writeFileSync(join(root,'fixture-panel'),'fixture');
    let output='';
    child=spawn('python3',[resolve('scripts/pty-panel.py'),binary],{env,stdio:['pipe','pipe','pipe']});
    child.on('error',error=>{output+=error.message;});
    child.stdout.on('data',s=>output+=s.toString());child.stderr.on('data',s=>output+=s.toString());
    const frame=()=>output.slice(output.lastIndexOf('\x1b[2J'));
    const text=()=>frame().replace(/\x1b\[[0-9;?]*[A-Za-z]/g,'').replace(/\s/g,'');
    const selected=()=>frame().match(/\x1b\[7m([^\x1b]*)/)?.[1]?.replace(/\s/g,'')??'';
    await until(()=>text().includes('m메뉴'),'panel did not render');
    const paths=['participation.json','live-summary.json','manual-day.json','retry.json','refresh.json','rebuild.json'];
    const before=Object.fromEntries(paths.map(p=>[p,existsSync(join(space,p))?readFileSync(join(space,p),'utf8'):null]));
    child.stdin.write('m');
    await until(()=>text().includes('기능선택'),'m menu missing');
    assert(selected().startsWith('저장기록보기'),'m menu default must be read-only records');
    child.stdin.write('\x1b');await until(()=>!text().includes('기능선택'),'m menu cancel did not render');
    for(const [p,value]of Object.entries(before))assert.equal(existsSync(join(space,p))?readFileSync(join(space,p),'utf8'):null,value,'opening m menu wrote '+p);
    child.stdin.write('\x1b[<2;4;4M\x1b[<2;4;4m'); // Right press/release at pane-local (3,3).
    await until(()=>text().includes('기능선택'),'right-click menu missing');
    assert(selected().startsWith('저장기록보기'),'menu default must be read-only records');
    assert(!existsSync(join(space,'participation.json')),'opening menu changed participation');
    if(action==='records') {
      child.stdin.write('\r');await until(()=>text().includes('고정된내용'),'default Enter did not open a frozen reader');
      child.stdin.write('svRrm\r');await pause(250);
      assert(text().includes('고정된내용')&&!text().includes('기능선택'),'m escaped the read-only reader');
      for(const [p,value]of Object.entries(before))assert.equal(existsSync(join(space,p))?readFileSync(join(space,p),'utf8'):null,value,'read-only records wrote '+p);
      child.stdin.write('\x1b');await until(()=>!text().includes('고정된내용'),'record Escape did not return');
      assert.equal(child.exitCode,null,'record Escape hid the panel');child.stdin.write('q');
    } else if(action==='cancel') {
      child.stdin.write('\x1b');await until(()=>!text().includes('기능선택'),'menu cancel did not render');
      assert.equal(child.exitCode,null,'Esc dismissed panel instead of menu');
      assert.equal(JSON.parse(readFileSync(join(space,'ui.json'))).visible,true);
      child.stdin.write('q');
    } else {
      const label=action==='disable'?'이탭수집끄기':'패널숨기기';
      for(let n=0;n<16&&!selected().startsWith(label);n++){
        const prior=selected();child.stdin.write('\x1b[B');await until(()=>selected()!==prior,'menu keyboard selection did not advance');
      }
      assert(selected().startsWith(label),'explicit action is not reachable');
      // Click the rendered selected row, rather than guessing its old item index.
      const prior=frame().slice(0,frame().indexOf('\x1b[7m'));
      const position=[...prior.matchAll(/\x1b\[(\d+);(\d+)H/g)].at(-1);
      assert(position,'selected menu cursor missing');const row=Number(position[1]),column=Number(position[2])+3;
      child.stdin.write(`\x1b[<0;${column};${row}M\x1b[<0;${column};${row}m`);
    }
    await until(()=>child.exitCode!==null,'panel action did not exit');
    assert.equal(child.exitCode,0,output.slice(-1500));
    const ui=JSON.parse(readFileSync(join(space,'ui.json')));
    assert.equal(ui.visible,false);
    const participation=existsSync(join(space,'participation.json'))?JSON.parse(readFileSync(join(space,'participation.json'))):{enabled:true};
    assert.equal(participation.enabled,action!=='disable');
    const calls=readFileSync(join(root,'herdr-calls.jsonl'),'utf8').trim().split('\n').map(JSON.parse);
    const routes=calls.filter(a=>a[0]==='pane'&&a[1]==='input');
    assert.deepEqual(routes.map(a=>a.at(-1)),['pane','herdr']);
    assert(routes.every(a=>a[3]==='fixture:pm'));
    assert.equal(calls.some(a=>a[0]==='pane'&&a[1]==='close'),['hide','disable'].includes(action));
    assert(output.includes('\x1b[?1006l'),'mouse capture not restored');
    console.log(`PASS ${action}: real PTY m and right-click, menu text, selection, exact participation effect, pane-only routing and cleanup`);
  } finally {
    if(child&&child.exitCode===null){child.stdin.write('q');await pause(500);if(child.exitCode===null)child.kill('SIGTERM');}
    rmSync(root,{recursive:true,force:true});
  }
}
