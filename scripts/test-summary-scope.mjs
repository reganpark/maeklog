// Explicit live opt-in only: three synthetic fixtures with isolated Maeklog settings.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,existsSync} from 'node:fs';
import {spawn} from 'node:child_process';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
if(process.argv[2]!=='--live') throw new Error('Opt in with --live /absolute/path/to/maeklog (uses isolated GPT-6 Luna Medium settings and live model calls)');
const binary=resolve(process.argv[3]??'target/debug/maeklog');
const root=mkdtempSync(join(tmpdir(),'pm-summary-scope-'));
const modelConfig=join(root,'maeklog-config');
mkdirSync(modelConfig,{mode:0o700});
writeFileSync(join(modelConfig,'model-settings.json'),JSON.stringify({model:'gpt-6-luna',effort:'medium'}),{mode:0o600});
const action=(text,step=0,when='지금')=>({owner:'sol',text,step,when,evidence:['event:1']});
const blocker=text=>({text,priority:'current',evidence:['event:1']});
const fixtures=[
  {key:'completed-report',previous:{
    objective:'PM 패널 검증',
    progress:[{text:'자동 테스트 실행',status:'reported',evidence:['event:1']}],
    blockers:[blocker('자동 테스트 통과 보고가 독립 검증되지 않았다.')],
    next_actions:[action('자동 테스트 실행'),action('실제 /new 세션 전환 검증')],
  },user:'자동 테스트와 실제 /new 전환 확인은 별개로 다뤄줘.',
    assistant:'자동 테스트 62개를 실행했고 모두 통과했습니다. 실제 /new 전환은 아직 실행하지 않았습니다.',
    check:s=>{
      assert(s.next_actions.some(a=>/new|세션.*전환/.test(a.text)),'remaining live test disappeared');
      assert(!s.next_actions.some(a=>/자동.*테스트/.test(a.text)),'completed automated test was recreated');
      assert(!s.blockers.some(b=>/독립/.test(b.text??b)),'absence of independent proof became a blocker');
      assert(s.progress.some(p=>p.status==='reported'),'reported completion lost its uncertainty label');
    }},
  {key:'optional-inactive',previous:{
    objective:'PM 패널의 최신 요약을 제공한다.',
    blockers:[blocker('비활성 Space의 MCP 연결을 확인할 수 없어 다음 실행까지 대기해야 한다.')],
    next_actions:[action('비활성 Space의 MCP 연결 확인')],
  },user:'현재는 PM 패널 작업을 진행하고, 비활성 Space의 MCP도 그 Space를 다음에 열면 확인해줘.',
    assistant:'PM 패널의 대화 수집과 요약은 정상 동작합니다. 해당 Space에는 실행 중인 세션이 없어 MCP 실행 연결은 다음에 확인해야 합니다.',
    check:s=>{
      assert(!s.blockers.some(b=>b.priority==='current'&&/MCP/.test(b.text)),'optional inactive MCP blocks current work');
      const actions=s.next_actions.filter(a=>/MCP/.test(a.text));assert(actions.length>0,'conditional MCP check disappeared');
      assert(actions.every(a=>a.step!==0&&a.when!=='지금'),'inactive-space check marked executable now');
    }},
  {key:'required-gate',previous:{objective:'보안 승인과 세 Space의 실제 세션 전환 검증을 마친 뒤 배포한다.',
    blockers:[{text:'배포 전에 사용자 보안 승인이 필요하다.',priority:'user_decision',evidence:['event:1']}],
    next_actions:[action('세 Space의 실제 /new 전환 확인'),action('보안 승인 후 배포',1,'승인 후')],
  },user:'배포 전 보안 승인은 필수야. 세 Space 모두 실제 /new 전환을 확인해야 해.',
    assistant:'자동 테스트는 통과했습니다. 실제 /new 전환은 대표 Space 한 곳에서만 확인했고, 나머지 두 Space는 미검증입니다. 보안 승인은 아직 받지 않았습니다.',
    check:s=>{
      assert(s.blockers.some(b=>/보안|승인/.test(b.text??b)),'required approval gate disappeared');
      assert(s.next_actions.some(a=>/두|2|나머지/.test(a.text)&&/Space|space|스페이스|세션|전환/.test(a.text)),'remaining scope was not narrowed');
      assert(!s.next_actions.some(a=>/배포/.test(a.text)&&a.step===0),'deployment marked executable without approval');
    }},
];
for(const f of fixtures){
  const dir=join(root,'spaces',f.key);mkdirSync(dir,{recursive:true,mode:0o700});
  const put=(name,v)=>writeFileSync(join(dir,name),JSON.stringify(v),{mode:0o600});
  put('meta.json',{space_key:f.key,workspace_id:f.key,session_name:'scope-test',cwd:root,label:f.key,created_at_unix:1,last_seen_at_unix:1,archived:false});
  put('current.json',{revision:1,base_revision:0,source_seq:0,updated_at_unix:1,summary:f.previous});
  // One attempt only; no hidden model retries from this evaluation command.
  put('retry.json',{key:'events:0',attempts:2,next_at:0});
  put('latest-retry.json',{key:'latest:2',attempts:3,next_at:0});
  const events=[f.user,f.assistant].map((content,i)=>({seq:i+1,kind:i?'assistant_final':'user_prompt',session_id:'scope-test',content,truncated:false,recorded_at_unix:i+1,cwd:root}));
  writeFileSync(join(dir,'inbox.jsonl'),events.map(JSON.stringify).join('\n')+'\n',{mode:0o600});
}
const env={...process.env,LC_ALL:'ko_KR.UTF-8',HERDR_PLUGIN_CONFIG_DIR:modelConfig,MAEKLOG_STATE_DIR:root,MAEKLOG_PLUGIN_ROOT:process.cwd()};
delete env.MAEKLOG_DISABLE_LUNA;delete env.MAEKLOG_INTERNAL;delete env.MAEKLOG_CODEX_BIN;
console.log(JSON.stringify({evaluation_root:root,cases:fixtures.map(f=>f.key),model:'gpt-6-luna',reasoning:'medium',service_tier:'default'}));
const child=spawn(binary,['worker'],{env,stdio:['ignore','ignore','inherit']});
const code=await new Promise((resolve,reject)=>{child.on('error',reject);child.on('exit',resolve);});
assert.equal(code,0,'evaluation worker failed');
const results=[];
for(const f of fixtures){
  const dir=join(root,'spaces',f.key);
  const state=JSON.parse(readFileSync(join(dir,'current.json'),'utf8'));
  const status=existsSync(join(dir,'status.json'))?JSON.parse(readFileSync(join(dir,'status.json'),'utf8')):null;
  let passed=false,error=null;
  try{assert.equal(state.source_seq,2,'model response failed or timed out');f.check(state.summary);passed=true;}catch(e){error=e.message;}
  results.push({case:f.key,passed,error,status,summary:state.summary});
}
writeFileSync(join(root,'results.json'),JSON.stringify(results,null,2),{mode:0o600});
console.log(JSON.stringify({results_path:join(root,'results.json'),results:results.map(({case:key,passed,error})=>({case:key,passed,error}))}));
process.exitCode=results.every(r=>r.passed)?0:1;
