#!/usr/bin/env node
// Client-routing fixture; never connects to a real Herdr server.
import {readFileSync,writeFileSync} from 'node:fs';
const path=process.env.PM_CLIENT_TOPOLOGY;
const state=JSON.parse(readFileSync(path,'utf8')),args=process.argv.slice(2);
if(process.env.HERDR_SOCKET_PATH!==state.socket)throw new Error('wrong Herdr endpoint');
const lookup=id=>state.panes.find(p=>p.pane_id===(state.aliases?.[id]??id));
let result;
if(args[0]==='pane'&&args[1]==='list')result={panes:state.panes};
else if(args[0]==='pane'&&args[1]==='current')result={pane:lookup(args[3]??process.env.HERDR_PANE_ID)};
else if(args[0]==='pane'&&args[1]==='get')result={pane:lookup(args[2])};
else if(args[0]==='pane'&&args[1]==='process-info') {
 const p=lookup(args[3]);
 result={process_info:{foreground_processes:p?[{pid:p.pid,name:p.process_name??'codex',argv:[p.process_name??'codex']}]:[]}};
} else if(args[0]==='pane'&&args[1]==='report-agent-session') {
 const p=lookup(args[2]);if(!p)throw new Error('missing pane');
 p.agent_session={kind:'id',value:args[args.indexOf('--agent-session-id')+1],source:'herdr:codex'};
 result={};
} else if(args[0]==='pane'&&args[1]==='release-agent') {
 const p=lookup(args[2]);if(!p)throw new Error('missing pane');delete p.agent_session;result={};
} else throw new Error('Unexpected call: '+JSON.stringify(args));
writeFileSync(path,JSON.stringify(state));console.log(JSON.stringify({result}));
