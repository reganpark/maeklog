#!/usr/bin/env node
// Isolated two-tab Herdr fixture; never contacts the real session.
import {appendFileSync,readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
const root=process.env.PM_TEST_ROOT,args=process.argv.slice(2),path=join(root,'tabs.json');
const state=JSON.parse(readFileSync(path,'utf8'));
appendFileSync(join(root,'herdr-calls.jsonl'),JSON.stringify(args)+'\n');
const mains=state.tabs.map(tab=>({workspace_id:'fixture',tab_id:tab,pane_id:tab.replace(':t',':p'),agent:'codex',agent_status:'idle',cwd:root,focused:tab===state.active,label:'Codex',agent_session:{kind:'id',value:'session-'+tab}}));
const panes=[...mains,...Object.values(state.panels)];
let result;
if(args[0]==='workspace'&&args[1]==='list') result={workspaces:[{workspace_id:'fixture',label:'Two tabs',active_tab_id:state.active}]};
else if(args[0]==='agent'&&args[1]==='list') result={agents:mains};
else if(args[0]==='pane'&&args[1]==='current') {const id=state.moved??process.env.HERDR_PANE_ID;result={pane:panes.find(p=>p.pane_id===id)};}
else if(args[0]==='pane'&&args[1]==='list') result={panes};
else if(args[0]==='pane'&&args[1]==='get') {result={pane:panes.find(p=>p.pane_id===args[2])};if(!result.pane)process.exit(1);}
else if(args[0]==='pane'&&args[1]==='process-info') result={process_info:{shell_pid:10,foreground_processes:[{pid:11,name:'maeklog',argv:[process.env.PM_TEST_BINARY,'panel']}]}};
else if(args[0]==='pane'&&args[1]==='input') result={};
else if(args[0]==='pane'&&args[1]==='close') {for(const [tab,pane] of Object.entries(state.panels))if(pane.pane_id===args[2])delete state.panels[tab];result={};}
else if(args.slice(0,3).join(' ')==='plugin pane open') {
 const target=panes.find(p=>p.pane_id===args[args.indexOf('--target-pane')+1]);
 if(!target)throw new Error('missing target');
 const panel=state.panels[target.tab_id]??{workspace_id:'fixture',tab_id:target.tab_id,pane_id:target.tab_id.replace(':t',':pm'),cwd:root,label:'Maeklog'};
 state.panels[target.tab_id]=panel;result={pane:panel};
} else throw new Error('Unexpected Herdr call: '+JSON.stringify(args));
writeFileSync(path,JSON.stringify(state));console.log(JSON.stringify({result}));
