#!/usr/bin/env node
// Isolated picker model/list fixture. Any inference method is a test failure.
import {appendFileSync,readFileSync} from 'node:fs';
import {createInterface} from 'node:readline';
const calls=process.env.MAEKLOG_PICKER_CALLS,fixture=process.env.MAEKLOG_PICKER_FIXTURE;
if(!calls||!fixture)throw Error('picker fixture paths required');
const log=value=>appendFileSync(calls,JSON.stringify({pid:process.pid,...value})+'\n');
const args=process.argv.slice(2);log({kind:'launch',args});
if(args[0]!=='app-server')throw Error('Picker attempted an inference invocation: '+JSON.stringify(args));
const reply=(id,result)=>process.stdout.write(JSON.stringify({id,result})+'\n');
const input=createInterface({input:process.stdin});
for await(const line of input){
 const request=JSON.parse(line);log({kind:'rpc',method:request.method,params:request.params});
 if(request.method==='initialize')reply(request.id,{userAgent:'picker-fixture'});
 else if(request.method==='initialized'){}
 else if(request.method==='config/read')reply(request.id,{config:{model_provider:'openai'},origins:{}});
 else if(request.method==='model/list'){
  const data=JSON.parse(readFileSync(fixture,'utf8'));
  if(data.delay_ms)await new Promise(done=>setTimeout(done,data.delay_ms));
  if(data.mode==='fail')process.stdout.write(JSON.stringify({id:request.id,error:{code:-32000,message:'offline fixture · 원문 오류'}})+'\n');
  else reply(request.id,{data:data.mode==='empty'?[]:data.models,nextCursor:null});
 }else throw Error('Unexpected picker RPC: '+request.method);
}
