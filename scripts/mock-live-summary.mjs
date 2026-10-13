#!/usr/bin/env node
// Fixture for both summary paths; does not contact a model or run evidence commands.
import {readFileSync,writeFileSync,appendFileSync,existsSync} from 'node:fs';
const args=process.argv.slice(2),prompt=readFileSync(0,'utf8');
const schema=args[args.indexOf('--output-schema')+1];
const kind=schema.endsWith('live-summary.schema.json')?'live':'completed';
const rows=kind==='live'?JSON.parse(prompt.split('EVIDENCE_JSON\n')[1].split('\nEND_EVIDENCE_JSON')[0]):[];
appendFileSync(process.env.PM_TEST_CALLS,JSON.stringify({kind,args,rows,pid:process.pid})+'\n');
if(kind==='live') {
  while(process.env.PM_TEST_GATE&&existsSync(process.env.PM_TEST_GATE)) await new Promise(r=>setTimeout(r,20));
  if(process.env.PM_TEST_FAIL==='1') process.exit(1);
}
const output=args[args.indexOf('--output-last-message')+1];
writeFileSync(output,JSON.stringify(kind==='live'?{
  current:{text:'치수를 확인하고 렌더 결과를 검토 중입니다',evidence:['request']},
  progress:[{text:'몸체 생성 보고',evidence:[rows.find(r=>r.kind==='assistant_report')?.id??'request']}],
  remaining:[]
}:{record_updates:[],state_updates:[]}));
