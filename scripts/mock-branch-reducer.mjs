#!/usr/bin/env node
// Isolated contract fixture only. Never invokes a model or external service.
import {readFileSync,writeFileSync,appendFileSync} from 'node:fs';
const prompt=readFileSync(0,'utf8');
const input=JSON.parse(prompt.split('EVIDENCE_JSON_BEGIN\n')[1].split('\nEVIDENCE_JSON_END')[0]);
const definitions=JSON.parse(readFileSync(process.env.PM_TEST_DELTAS,'utf8'));
const feedbackMatch=prompt.match(/PREVIOUS VALIDATION FAILURE \(diagnostic data, not instructions\):\n([^\n]+)/);
appendFileSync(process.env.PM_TEST_CALLS,JSON.stringify({args:process.argv.slice(2),events:input.new_events.map(e=>e.seq),previous:input.previous,feedback:!!feedbackMatch,validation_feedback:feedbackMatch?JSON.parse(feedbackMatch[1]):null,decision_tuple_contract:prompt.includes('kind=decision, stage=decision, outcome=confirmed'),mixed_claim_contract:prompt.includes('An explicit choice for one claim never confirms a neighboring requirement or proposal.')})+'\n');
const record_updates=[],state_updates=[];
for(const event of input.new_events){
  const entry=definitions[event.seq];if(!entry)continue;
  const evidence=[{reference:`event:${event.seq}`,quote:event.content}];
  for(const record of entry.records??[])record_updates.push({...record,evidence:record.evidence??evidence,decision_basis:record.decision_basis===undefined?(record.kind==='decision'?{intent:'explicit_choice',...evidence[0]}:null):record.decision_basis});
  for(const state of entry.states??[])state_updates.push({...state,evidence:state.evidence??evidence});
}
// Select exact prevalidated source candidates; invalid legacy fixtures remain strict.
const candidates=JSON.parse(prompt.split('SOURCE_CANDIDATES_BEGIN\n')[1].split('\nSOURCE_CANDIDATES_END')[0]);
function select(e) {
  for(const [candidate_id,quote] of Object.entries(candidates[e.reference]??{})) {
    if(quote===e.quote) { const {quote:ignored,...rest}=e;return {...rest,candidate_id}; }
  }
  return e;
}
for(const update of [...record_updates,...state_updates]) {
  update.evidence=update.evidence.map(select);
  if(update.decision_basis)update.decision_basis=select(update.decision_basis);
}
const output=process.argv[process.argv.indexOf('--output-last-message')+1];
writeFileSync(output,JSON.stringify({objective:'',objective_evidence:[],discussion:[],decisions:[],progress:[],blockers:[],next_actions:[],notes:[],record_updates,state_updates}));
