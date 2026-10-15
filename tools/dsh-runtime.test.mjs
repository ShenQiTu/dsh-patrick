import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
import {createServer} from 'node:http';
import path from 'node:path';
const source=process.env.DSH_PATRICK_DSH_SOURCE||assert.fail('Set DSH_PATRICK_DSH_SOURCE to the active DSH source');
const req=createRequire(path.join(source,'packages/llm/llm-pi-ai/package.json'));
const load=async name=>import(pathToFileURL(req.resolve(name)).href);
const {Context}=await load('@deepseek-ai/cordis');
const {default:LlmRuntime,BlockAssembler,createUserMessage}=await load('@deepseek-ai/dsh-llm');
const Pi=await load('@deepseek-ai/dsh-llm-pi-ai');
const zenPath=path.join(source,'packages/llm/llm-zen-session-headers/lib/index.js');
const Zen=await import(pathToFileURL(zenPath).href);
const require=createRequire(import.meta.url);
const {generate,yamlTools}=require('../src-tauri/src/dsh_bridge.cjs');
const y=yamlTools();
// Synthetic declarations only, never parse the real DSH configuration here.
const doc=y.parse('- id: llm-pi-ai\n  config:\n    providers: {}\n- id: llm-zen-session-headers\n  config: {enabled: true, models: [zen-free], providers: []}\n');
const observed=[];
const server=createServer((request,response)=>{
  let body='';request.on('data',s=>body+=s);request.on('end',()=>{
    observed.push({path:request.url,headers:request.headers,body:JSON.parse(body)});
    const send=(name,value)=>response.write(`${name?'event: '+name+'\n':''}data: ${JSON.stringify(value)}\n\n`);
    response.writeHead(200,{'content-type':'text/event-stream'});
    if(request.url==='/v1/messages?beta=true') {
      send('message_start',{type:'message_start',message:{id:'msg_test',type:'message',role:'assistant',model:'mock',content:[],stop_reason:null,usage:{input_tokens:3,output_tokens:0}}});
      send('content_block_start',{type:'content_block_start',index:0,content_block:{type:'text',text:''}});
      send('content_block_delta',{type:'content_block_delta',index:0,delta:{type:'text_delta',text:'hello'}});
      send('content_block_stop',{type:'content_block_stop',index:0});
      send('message_delta',{type:'message_delta',delta:{stop_reason:'end_turn'},usage:{output_tokens:1}});
      send('message_stop',{type:'message_stop'});
    } else if(request.url==='/v1/responses') {
      const item={type:'message',id:'msg_test',role:'assistant',status:'completed',content:[{type:'output_text',text:'hello',annotations:[]}]};
      send('response.created',{type:'response.created',response:{id:'resp_test',status:'in_progress',output:[]}});
      send('response.output_item.added',{type:'response.output_item.added',output_index:0,item:{...item,status:'in_progress',content:[]}});
      send('response.content_part.added',{type:'response.content_part.added',item_id:'msg_test',output_index:0,content_index:0,part:{type:'output_text',text:'',annotations:[]}});
      send('response.output_text.delta',{type:'response.output_text.delta',item_id:'msg_test',output_index:0,content_index:0,delta:'hello'});
      send('response.output_item.done',{type:'response.output_item.done',output_index:0,item});
      send('response.completed',{type:'response.completed',response:{id:'resp_test',status:'completed',output:[item],usage:{input_tokens:3,output_tokens:1,total_tokens:4}}});
    } else {
      send('',{choices:[{index:0,delta:{role:'assistant',content:'hello'},finish_reason:null}]});
      send('',{choices:[{index:0,delta:{},finish_reason:'stop'}],usage:{prompt_tokens:3,completion_tokens:1}});
      response.write('data: [DONE]\n\n');
    }
    response.end();
  });
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const input=process.argv[2]?JSON.parse(process.argv[2]):null;
const port=input?.port??server.address().port;
const models=input?.models??[{id:'pzen',name:'Zen',model:'zen-free',protocol:'openai'},{id:'pnormal',name:'Normal',model:'ordinary',protocol:'openai'},{id:'presponses',name:'Responses',model:'responses',protocol:'responses'},{id:'panthropic',name:'Anthropic',model:'anthropic',protocol:'anthropic'}];
const generated=generate(y,doc,{models,port},'DSH_PATRICK_RUNTIME_TEST');
process.env.DSH_PATRICK_RUNTIME_TEST=input?.token??'patrick_'+'b'.repeat(64);
const ctx=new Context();let checks=0;
try {
  assert(req.resolve('@deepseek-ai/dsh-llm-pi-ai').startsWith(source));checks++;
  await ctx.plugin(LlmRuntime);await ctx.plugin(Pi,{providers:generated.providers});await ctx.plugin(Zen,{enabled:true,providers:[],models:generated.zenIds});
  for(const model of models) {
    const route='dsh-patrick-'+model.protocol;
    const catalog=await ctx.llm.listModels(route);assert(catalog.some(m=>m.id===model.id));checks++;
    for(const sessionId of ['session-A','session-A','session-B']) {
      const assembler=new BlockAssembler();
      for await(const chunk of ctx.llm.stream({provider:route,model:model.id,sessionId,messages:[createUserMessage({content:[{type:'text',text:'local fixture'}]})]}))assembler.push(chunk);
      assert.equal(assembler.finish.kind,'stop',JSON.stringify({finish:assembler.finish,paths:observed.map(x=>x.path)}));checks++;
    }
  }
  if(!input) {
    assert.equal(observed.length,12);checks++;
    for(const m of models) {
      const requests=observed.filter(r=>r.body.model===m.id);assert.equal(requests.length,3);checks++;
      const expected=m.protocol==='anthropic'?'/v1/messages?beta=true':m.protocol==='responses'?'/v1/responses':'/v1/chat/completions';
      assert(requests.every(r=>r.path===expected));checks++;
      assert(requests.every(r=>r.headers['x-dsh-patrick']==='1'));checks++;
      assert(requests.every(r=>r.headers.authorization==='Bearer '+process.env.DSH_PATRICK_RUNTIME_TEST||r.headers['x-api-key']===process.env.DSH_PATRICK_RUNTIME_TEST));checks++;
      if(m.id==='pzen') {const h=requests.map(r=>r.headers['x-opencode-session']);assert(/^ses_[0-9a-f]{12}[0-9A-Za-z]{14}$/.test(h[0]));assert.equal(h[0],h[1]);assert.notEqual(h[0],h[2]);checks+=3;}
      else {assert(requests.every(r=>!r.headers['x-opencode-session']));checks++;}
    }
  }
  console.log(`DSH_RUNTIME PASS checks=${checks} protocols=${Object.keys(generated.providers).length} loopback_only=YES exit=0`);
} finally {
  await ctx.fiber.dispose();server.closeAllConnections();await new Promise(r=>server.close(r));
}
