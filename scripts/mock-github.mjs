#!/usr/bin/env node
// Read-only GitHub responses; unexpected verbs fail the test, never reach gh.
// Repository/head-specific lists let one panel test branch changes without
// accidentally accepting a query for the previous branch. Directives exercise
// failures, malformed responses and delayed background fetches locally.
import {readFileSync,appendFileSync} from 'node:fs';
const args=process.argv.slice(2);
appendFileSync(process.env.PM_TEST_GH_CALLS,JSON.stringify(args)+'\n');
const rest=args[0]==='api';
if(rest){
  if(args.length!==4||args[1]!=='--method'||args[2]!=='GET'||!/^repos\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/issues\/[1-9][0-9]*$/.test(args[3]))throw Error('Not an exact read-only issue/PR lookup');
}else if(!['issue','pr'].includes(args[0])||!['view','list'].includes(args[1]))throw Error('Not a read-only query');
if(args.includes('--web'))throw Error('Browser access is not a metadata query');
if(process.env.PM_TEST_GH_FAIL==='1')process.exit(1);
const fixture=JSON.parse(readFileSync(process.env.PM_TEST_GH_FIXTURE,'utf8'));
const option=name=>args.includes(name)?args[args.indexOf(name)+1]:undefined;
const restParts=rest?args[3].split('/'):null;
const repository=rest?restParts.slice(1,3).join('/'):option('--repo');
if(!repository)throw Error('An exact repository is required');
const keys=rest?['api:'+repository+'#'+restParts[4],'issue:'+repository+'#'+restParts[4],'pr:'+repository+'#'+restParts[4]]:args[1]==='list'
  ?['list:'+repository+'#'+option('--head'),'list:'+repository,'list']
  :[args[0]+':'+repository+'#'+args[2]];
const key=keys.find(key=>Object.hasOwn(fixture,key));
if(!key)throw Error('Unknown fixture query '+keys[0]);
const response=fixture[key];
const delay=Number(response?.__delay_ms??process.env.PM_TEST_GH_DELAY_MS??0);
if(delay>0)await new Promise(resolve=>setTimeout(resolve,delay));
if(response?.__exit!==undefined){
  if(response.__stderr)process.stderr.write(response.__stderr);
  process.exit(response.__exit);
}
if(response?.__raw!==undefined)process.stdout.write(response.__raw);
else{
  let value=response?.__response??response;
  // Legacy view fixtures can also serve the REST probe. Explicit api fixtures
  // retain their exact shape so identity/type errors remain observable.
  if(rest&&!key.startsWith('api:')){
    value={...value,html_url:value.html_url??value.url};
    if(key.startsWith('pr:'))value.pull_request=value.pull_request??{html_url:value.url};
  }
  console.log(JSON.stringify(value));
}
