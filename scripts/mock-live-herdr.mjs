#!/usr/bin/env node
// Isolated progress test fixture; never connects to a real Herdr socket.
import {readFileSync} from 'node:fs';
const args=process.argv.slice(2);
if(args[0]==='pane'&&args[1]==='input') {
  console.log(JSON.stringify({result:{}}));
} else if(args[0]==='agent'&&args[1]==='list') {
  const topology=JSON.parse(readFileSync(process.env.PM_CLIENT_TOPOLOGY,'utf8'));
  console.log(JSON.stringify({result:{agents:topology.panes.map(p=>({...p,agent:'codex',agent_status:'working',terminal_title_stripped:'테스트 작업'}))}}));
} else await import('./mock-client-herdr.mjs');
