const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {run,yamlTools}=require(path.resolve(process.argv[2]||'src-tauri/src/dsh_bridge.bundle.cjs'));
const y=yamlTools(),base=path.resolve(process.env.DSH_PATRICK_TEST_ROOT||assert.fail('isolated directory required'));
assert(!fs.existsSync(base));fs.mkdirSync(base,{recursive:true});let n=0,checks=0;
const check=(ok,name)=>{assert(ok,name);checks++;console.log('PASS '+name);};
const route='dsh-patrick-openai';
const policy={mode:'normal',maxRetries:5,retryableCodes:['EMPTY_RESPONSE','RATE_LIMIT','SERVER','TIMEOUT','TRANSPORT','UPSTREAM_HTTP_400']};
const original='# original comment\n- id: llm-pi-ai\n  config:\n    providers:\n      direct:\n        api: openai-completions\n        baseURL: https://example.invalid/v1\n        models: [{id: original}]\n        retryPolicy: {mode: normal, maxRetries: 2}\n- id: default-model\n  config: {model: keep}\n';
const creds='version: 1\nrefs: {OLD: SYNTHETIC_ONLY}\n';
function fixture() {
  const home=path.join(base,'case-'+(++n)),profile=path.join(home,'profiles/web');fs.mkdirSync(profile,{recursive:true});
  const f=path.join(profile,'cordis.patch.yml'),c=path.join(home,'.credentials.yaml'),j=path.join(profile,'.dsh-patrick/journal.json'),o=path.join(profile,'.dsh-patrick/provider-options.json');
  fs.writeFileSync(path.join(profile,'package.json'),'{}');fs.writeFileSync(f,original);fs.writeFileSync(c,creds);
  return {f,c,j,o,input:{action:'sync',profile,owner:'synthetic-owner',pid:process.pid,port:48787,token:'patrick_'+'a'.repeat(64),models:[{id:'local',name:'Synthetic',model:'synthetic',protocol:'openai',contextWindow:64000,maxOutput:2048}]}};
}
const doc=t=>y.parse(fs.readFileSync(t.f,'utf8'));
const provider=t=>y.providers(doc(t)).get(route)?.toJSON();
function edit(t,fn) {const d=doc(t);fn(y.providers(d).get(route),d);fs.writeFileSync(t.f,String(d));}
const files=t=>[t.f,t.c,t.j,t.o].map(f=>fs.existsSync(f)?fs.readFileSync(f,'utf8'):null);
async function attach(){const t=fixture();await run(t.input);edit(t,p=>p.set('retryPolicy',doc(t).createNode(policy)));return t;}
(async()=>{
  const t=await attach(),before=files(t);
  try {check((await run({...t.input,action:'status'})).configured,'external_retry_policy_status_synced');}
  catch(e) {console.log('BASELINE_REPRODUCED='+e.message);throw e;}
  check(JSON.stringify(files(t))===JSON.stringify(before),'status_is_read_only');
  await run({...t.input,models:[{...t.input.models[0],name:'Renamed'}]});
  check(JSON.stringify(provider(t).retryPolicy)===JSON.stringify(policy)&&provider(t).models[0].name==='Renamed','resync_preserves_retry_and_updates_owned_model');
  check(y.providers(doc(t)).get('direct').getIn(['retryPolicy','maxRetries'])===2,'direct_provider_unchanged');
  check((await run({...t.input,action:'status'})).configured,'resync_new_journal_synced');
  edit(t,p=>p.setIn(['retryPolicy','maxRetries'],7));await run(t.input);
  check(provider(t).retryPolicy.maxRetries===7,'later_external_policy_edit_preserved');
  await run({...t.input,action:'disconnect'});
  check(JSON.stringify(doc(t).toJSON())===JSON.stringify(y.parse(original).toJSON())&&fs.readFileSync(t.f,'utf8').includes('# original comment')&&fs.readFileSync(t.c,'utf8')===creds&&!fs.existsSync(t.j),'disconnect_restores_original_config_and_preserves_comments_credentials');
  check(JSON.parse(fs.readFileSync(t.o)).providers[route].maxRetries===7,'disconnect_archives_policy_only');
  await run(t.input);check(provider(t).retryPolicy.maxRetries===7,'reconnect_preserves_policy');
  await run({...t.input,models:[]});check(!provider(t),'disable_all_removes_provider');
  await run(t.input);check(provider(t).retryPolicy.maxRetries===7,'enable_after_disable_preserves_policy');
  edit(t,p=>p.delete('retryPolicy'));await run(t.input);check(provider(t).retryPolicy===undefined,'explicit_policy_removal_not_resurrected');
  await run({...t.input,action:'disconnect'});await run(t.input);check(provider(t).retryPolicy===undefined,'explicit_removal_survives_reconnect');
  const always={mode:'always',maxRetries:0,retryableCodes:[],backoff:{initialDelayMs:100,maxDelayMs:1000,jitterRatio:0}};
  edit(t,p=>p.set('retryPolicy',doc(t).createNode(always)));await run(t.input);check(JSON.stringify(provider(t).retryPolicy)===JSON.stringify(always),'existing_always_policy_and_inactive_fields_preserved');
  for(const [field,value] of Object.entries({baseURL:'https://example.invalid',apiKeyEnv:'OTHER_REF',headers:{'x-dsh-patrick':'other'},api:'openai-responses',models:[{id:'changed'}],streamIdleTimeoutMs:1,displayName:'changed',unknownField:true})) {
    const c=await attach();edit(c,p=>p.set(field,doc(c).createNode(value)));const prior=files(c);
    for(const action of ['status','sync','disconnect'])await assert.rejects(()=>run({...c.input,action}),/外部修改/);
    check(JSON.stringify(files(c))===JSON.stringify(prior),'critical_'+field+'_still_rejected_without_overwrite');
  }
  for(const bad of [null,[],{mode:'invalid'},{mode:'normal',maxRetries:-1},{mode:'normal',maxRetries:1.5},{mode:'normal',retryableCodes:[]},{mode:'normal',retryableCodes:['A','A']},{mode:'normal',retryableCodes:[1]},{mode:'normal',unexpected:true},{mode:'normal',backoff:{initialDelayMs:0}},{mode:'normal',backoff:{initialDelayMs:1001,maxDelayMs:1000}},{mode:'normal',backoff:{jitterRatio:2}},{mode:'normal',backoff:{unknown:1}}]) {
    const c=await attach();edit(c,p=>p.set('retryPolicy',doc(c).createNode(bad)));const prior=files(c);
    for(const action of ['status','sync','disconnect'])await assert.rejects(()=>run({...c.input,action}),/retryPolicy/);
    check(JSON.stringify(files(c))===JSON.stringify(prior),'invalid_policy_rejected_without_writes_'+(++n));
  }
  const unrelated=await attach();edit(unrelated,(p,d)=>y.row(d,'default-model').setIn(['config','model'],'user-edit'));await run({...unrelated.input,action:'disconnect'});
  check(y.row(doc(unrelated),'default-model').getIn(['config','model'])==='user-edit'&&!provider(unrelated),'disconnect_preserves_unrelated_external_edits');
  const fail=await attach(),initial=files(fail),rename=fs.renameSync;
  fs.renameSync=(a,b)=>{if(b===fail.f)throw Error('synthetic profile failure');return rename(a,b);};
  try {await assert.rejects(()=>run(fail.input),/synthetic profile/);}finally {fs.renameSync=rename;}
  check(files(fail).slice(0,3).every((v,i)=>v===initial[i]),'partial_sync_restores_profile_credentials_journal');
  await run(fail.input);check(provider(fail).retryPolicy.maxRetries===5,'retry_after_partial_failure_preserves_policy');
  const cacheFailure=await attach(),cb=files(cacheFailure);fs.renameSync=(a,b)=>{if(b===cacheFailure.o)throw Error('synthetic cache failure');return rename(a,b);};
  try {await assert.rejects(()=>run({...cacheFailure.input,action:'disconnect'}),/synthetic cache/);}finally {fs.renameSync=rename;}
  check(JSON.stringify(files(cacheFailure))===JSON.stringify(cb),'cache_failure_leaves_live_integration_unchanged');
  const corrupt=await attach();await run(corrupt.input);fs.writeFileSync(corrupt.o,'{');const corruptBefore=files(corrupt);
  await assert.rejects(()=>run({...corrupt.input,action:'disconnect'}),/保留记录/);check(JSON.stringify(files(corrupt))===JSON.stringify(corruptBefore),'corrupt_cache_no_overwrite');
  const pending=await attach();await run(pending.input);const old=JSON.parse(fs.readFileSync(pending.j));
  await run({...pending.input,models:[{...pending.input.models[0],name:'new'}]});const next=JSON.parse(fs.readFileSync(pending.j));next.phase='pending';next.previous=old;fs.writeFileSync(pending.j,JSON.stringify(next));fs.writeFileSync(pending.f,old.after.profile);
  await run({...pending.input,action:'disconnect'});await run(pending.input);check(provider(pending).retryPolicy.maxRetries===5,'pending_recovery_preserves_retry_policy');
  console.log(`DSH_RETRY_PASS checks=${checks} formal_dsh=UNCHANGED requests=NONE`);
})().catch(e=>{console.error(e.message);process.exitCode=1;});
