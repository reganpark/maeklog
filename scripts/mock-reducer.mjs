#!/usr/bin/env node
// Only for test-recovery.mjs: never contacts an AI service.
import {readFileSync, appendFileSync, writeFileSync, existsSync} from 'node:fs';
import {basename, join} from 'node:path';
const prompt = readFileSync(0, 'utf8');
const evidence = JSON.parse(prompt.split('EVIDENCE_JSON_BEGIN\n')[1].split('\nEVIDENCE_JSON_END')[0]);
if(process.env.PM_TEST_EVIDENCE) appendFileSync(process.env.PM_TEST_EVIDENCE,JSON.stringify(evidence)+'\n');
appendFileSync(process.env.PM_TEST_CALLS, JSON.stringify(evidence.new_events.map(e => e.seq)) + '\n');
const space = basename(evidence.new_events[0]?.cwd ?? 'unknown');
const trace = type => {
  if(process.env.PM_TEST_TRACE) appendFileSync(process.env.PM_TEST_TRACE,
    JSON.stringify({type,space,pid:process.pid,worker:process.ppid,at:Date.now(),seq:evidence.new_events.map(e=>e.seq)})+'\n');
};
trace('start');
while(process.env.PM_TEST_GATE_DIR && existsSync(join(process.env.PM_TEST_GATE_DIR,space))) {
  await new Promise(resolve=>setTimeout(resolve,20));
}
if (process.env.PM_TEST_FAIL === '1' || (process.env.PM_TEST_FAIL_SPACES??'').split(',').includes(space)) {trace('end');process.exit(1);}
const output = process.argv[process.argv.indexOf('--output-last-message') + 1];
const actions=process.env.PM_TEST_TIMING_CONFLICT==='1' ? [{owner:process.env.PM_TEST_BAD_OWNER==='1'?'invalid':'sol',text:'보존할 구현 행동',evidence:[`event:${evidence.new_events.at(-1).seq}`],step:0,when:'승인 후'}] : [];
const supportingEvidence=actions.length ? [`event:${evidence.new_events.at(-1).seq}`] : [];
const decisions=actions.length ? [{id:'D-001',text:'보존할 정상 결정',status:'proposed',supersedes:null,evidence:supportingEvidence}] : [];
const progress=actions.length ? [{text:'보존할 정상 진행',status:'reported',evidence:supportingEvidence}] : [];
writeFileSync(output, JSON.stringify({objective:'테스트 프로젝트 목표',objective_evidence:[],discussion:[],decisions,progress,blockers:[],next_actions:actions,notes:[]}));
trace('end');
