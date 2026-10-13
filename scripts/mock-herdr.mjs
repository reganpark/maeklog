#!/usr/bin/env node
// Opt-out integration fixture only. Never talks to a Herdr server.
import {appendFileSync,existsSync,writeFileSync,unlinkSync} from 'node:fs';
import {join} from 'node:path';
const root=process.env.PM_TEST_ROOT;
if(!root) throw new Error('PM_TEST_ROOT required');
const args=process.argv.slice(2), space=process.env.HERDR_WORKSPACE_ID;
appendFileSync(join(root,'herdr-calls.jsonl'),JSON.stringify(args)+'\n');
const marker=join(root,space+'-panel');
const main={pane_id:space+':main',agent:'codex',cwd:root,label:'Codex'};
const panel={pane_id:space+':pm',label:'Maeklog',cwd:root};
let result;
if(args[0]==='workspace'&&args[1]==='list') result={workspaces:[{workspace_id:space,label:'Fixture'}]};
else if(args[0]==='agent'&&args[1]==='list') result={agents:[]};
else if(args[0]==='pane'&&args[1]==='list') result={panes:[main,...(existsSync(marker)?[panel]:[])]};
else if(args[0]==='pane'&&args[1]==='get'&&args[2]===panel.pane_id&&existsSync(marker)) result={pane:panel};
else if(args[0]==='pane'&&args[1]==='process-info') result={process_info:{shell_pid:10,foreground_processes:[{pid:11,name:'maeklog',argv:[process.env.PM_TEST_BINARY,'panel']}]}};
else if(args[0]==='pane'&&args[1]==='input') result={};
else if(args[0]==='pane'&&args[1]==='close'&&args[2]===panel.pane_id) {unlinkSync(marker);result={};}
else if(args.slice(0,3).join(' ')==='plugin pane open') {writeFileSync(marker,'fixture');result={pane:panel};}
else throw new Error('Unexpected fixture Herdr call: '+JSON.stringify(args));
console.log(JSON.stringify({result}));
