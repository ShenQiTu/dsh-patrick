const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {createRequire}=require('node:module');
const {run,yamlTools}=require('../src-tauri/src/dsh_bridge.cjs');
const base=path.resolve(process.env.DSH_PATRICK_TEST_ROOT||assert.fail('Set DSH_PATRICK_TEST_ROOT to an isolated directory'));
assert(!fs.existsSync(base));fs.mkdirSync(base,{recursive:true});
// Only the parser is reused from the installed profile. No plugin or real configuration is loaded.
const parser=createRequire(process.env.DSH_PATRICK_YAML_ANCHOR||assert.fail('Set DSH_PATRICK_YAML_ANCHOR')).resolve('yaml');
fs.mkdirSync(path.join(base,'node_modules/yaml'),{recursive:true});
fs.writeFileSync(path.join(base,'node_modules/yaml/package.json'),JSON.stringify({main:parser}));
let checks=0;
function check(value,name){assert(value,name);checks++;console.log('PASS '+name);}
const original=`# preserve this comment\n- id: unrelated\n  config:\n    expression: !!js process.env.LOCAL_SAMPLE\n- id: llm-pi-ai\n  config:\n    providers:\n      old:\n        api: openai-completions\n        apiKeyEnv: OLD_KEY\n        baseURL: https://example.invalid/v1\n        models:\n          - id: zen-free\n            input: [text, image]\n            reasoningEfforts: {off: null, high: high}\n- insert:\n    - id: llm-zen-session-headers\n      name: '@deepseek-ai/dsh-llm-zen-session-headers'\n      config:\n        enabled: true\n        providers: []\n        models: [zen-*]\n- id: agent-default-model\n  config: {provider: old, model: untouched}\n`;
const creds='version: 1\nrefs:\n  OLD_KEY: TEST_ONLY_NOT_A_REAL_KEY\nrecords: {}\n';
let number=0;
function fixture() {
  const home=path.join(base,'fixture-'+(++number)),profile=path.join(home,'profiles/web');fs.mkdirSync(profile,{recursive:true});fs.writeFileSync(path.join(profile,'package.json'),'{}');
  const f=path.join(profile,'cordis.patch.yml'),c=path.join(home,'.credentials.yaml');fs.writeFileSync(f,original);fs.writeFileSync(c,creds);
  const input={action:'sync',profile,owner:'isolated-owner',pid:process.pid,port:48787,token:'patrick_'+'a'.repeat(64),models:[{id:'pzen',name:'Zen',model:'zen-free',protocol:'openai',contextWindow:64000,maxOutput:2048},{id:'pnormal',name:'Normal',model:'ordinary',protocol:'responses'},{id:'panthropic',name:'Anthropic',model:'claude-fixture',protocol:'anthropic'}]};
  return {input,profile,f,c,j:path.join(profile,'.dsh-patrick/journal.json'),y:yamlTools(profile)};
}
async function rejects(fn,pattern,name){await assert.rejects(fn,pattern);check(true,name);}
(async()=>{
  const t=fixture();await run({...t.input,action:'inspect'});check(fs.readFileSync(t.f,'utf8')===original&&!fs.existsSync(t.j),'inspect_read_only');
  const result=await run(t.input);check(result.configured&&result.modelCount===3&&result.zenCount===1,'sync_three_protocols');
  let d=t.y.parse(fs.readFileSync(t.f,'utf8')),providers=t.y.providers(d).toJSON();
  check(providers.old.apiKeyEnv==='OLD_KEY'&&t.y.row(d,'agent-default-model').getIn(['config','model'])==='untouched','original_provider_and_default_unchanged');
  check(providers['dsh-patrick-anthropic'].baseURL==='http://127.0.0.1:48787','anthropic_no_double_v1');
  check(providers['dsh-patrick-responses'].api==='openai-responses'&&providers['dsh-patrick-openai'].api==='openai-completions','explicit_protocols');
  check(providers['dsh-patrick-openai'].models[0].id==='pzen'&&providers['dsh-patrick-openai'].models[0].input.includes('image'),'unique_local_id_and_known_capability');
  check(providers['dsh-patrick-openai'].models[0].contextWindow===64000&&providers['dsh-patrick-openai'].models[0].maxTokens===2048,'explicit_limits');
  const z=t.y.row(d,'llm-zen-session-headers').getIn(['config','models']).toJSON();check(z.includes('pzen')&&!z.includes('pnormal'),'zen_only_matching_models');
  check(fs.readFileSync(t.f,'utf8').includes('!!js')&&fs.readFileSync(t.f,'utf8').includes('preserve this comment'),'yaml_tag_comment_preserved');
  check(!JSON.stringify(Object.entries(providers).filter(([k])=>k.startsWith('dsh-patrick'))).includes('example.invalid'),'upstream_addresses_not_copied');
  check((await run({...t.input,action:'status'})).configured,'status_verified');
  await run({...t.input,models:[{...t.input.models[1],name:'Renamed'}]});d=t.y.parse(fs.readFileSync(t.f,'utf8'));check(!t.y.providers(d).has('dsh-patrick-openai')&&!t.y.row(d,'llm-zen-session-headers').getIn(['config','models']).toJSON().includes('pzen'),'resync_removes_disabled_routes_and_zen');
  await run({...t.input,models:[]});check(t.y.providers(t.y.parse(fs.readFileSync(t.f,'utf8'))).items.length===1,'zero_enabled_models');
  await run({...t.input,action:'disconnect'});check(fs.readFileSync(t.f,'utf8')===original&&fs.readFileSync(t.c,'utf8')===creds&&!fs.existsSync(t.j),'exact_bytes_rollback');
  await run(t.input);d=t.y.parse(fs.readFileSync(t.f,'utf8'));t.y.row(d,'agent-default-model').setIn(['config','model'],'user-change');fs.writeFileSync(t.f,String(d));
  let c=t.y.parse(fs.readFileSync(t.c,'utf8'));c.setIn(['refs','OTHER_NEW'],'OTHER_TEST_KEY');fs.writeFileSync(t.c,String(c));
  await run({...t.input,action:'disconnect'});d=t.y.parse(fs.readFileSync(t.f,'utf8'));check(t.y.row(d,'agent-default-model').getIn(['config','model'])==='user-change'&&t.y.providers(d).items.length===1,'concurrent_unrelated_edit_preserved');check(t.y.parse(fs.readFileSync(t.c,'utf8')).getIn(['refs','OTHER_NEW'])==='OTHER_TEST_KEY','unrelated_credential_preserved');
  const conflict=fixture();await run(conflict.input);let cd=conflict.y.parse(fs.readFileSync(conflict.f,'utf8'));conflict.y.providers(cd).get('dsh-patrick-openai').set('displayName','external');fs.writeFileSync(conflict.f,String(cd));
  const hashBefore=fs.readFileSync(conflict.f,'utf8');await rejects(()=>run({...conflict.input,action:'disconnect'}),/外部修改/,'owned_change_conflict');check(fs.readFileSync(conflict.f,'utf8')===hashBefore&&fs.existsSync(conflict.j),'conflict_no_overwrite');
  const collision=fixture();await run(collision.input);await rejects(()=>run({...collision.input,owner:'different'}),/另一份/,'different_owner_rejected');await rejects(()=>run({...collision.input,pid:4}),/另一 Patrick/,'live_instance_rejected');
  const pending=fixture();await run(pending.input);const old=JSON.parse(fs.readFileSync(pending.j));await run({...pending.input,models:[pending.input.models[1]]});let newer=JSON.parse(fs.readFileSync(pending.j));newer.phase='pending';newer.previous=old;newer.routes=old.routes;newer.zenIds=old.zenIds;fs.writeFileSync(pending.j,JSON.stringify(newer));fs.writeFileSync(pending.f,old.after.profile);await run({...pending.input,action:'disconnect'});check(fs.readFileSync(pending.f,'utf8')===original&&fs.readFileSync(pending.c,'utf8')===creds,'crash_during_resync_recovery');
  const failwrite=fixture();const rename=fs.renameSync;fs.renameSync=(a,b)=>{if(b===failwrite.f)throw new Error('injected disk failure');return rename(a,b);};try {await rejects(()=>run(failwrite.input),/injected/,'partial_write_failure');}finally{fs.renameSync=rename;}
  check(fs.readFileSync(failwrite.c,'utf8')===creds&&fs.readFileSync(failwrite.f,'utf8')===original&&!fs.existsSync(failwrite.j),'failed_attach_rolled_back');
  const missing=fixture();fs.writeFileSync(missing.f,'- id: other\n  config: {}\n');await rejects(()=>run(missing.input),/llm-pi-ai/,'unknown_layout_rejected');
  const invalid=fixture();fs.writeFileSync(invalid.f,'[');await rejects(()=>run(invalid.input),/解析失败/,'malformed_yaml_rejected');
  const overlay=fixture();fs.writeFileSync(path.join(path.dirname(path.dirname(overlay.profile)),'cordis.patch.yml'),'- id: llm-pi-ai\n  config: {}\n');await rejects(()=>run(overlay.input),/overlay/,'shadowing_overlay_rejected');
  const locked=fixture();fs.writeFileSync(path.join(locked.profile,'package.json.lock'),process.pid+'\n');await rejects(()=>run(locked.input),/写锁繁忙/,'respects_dsh_writer_lock');check(fs.readFileSync(locked.f,'utf8')===original,'locked_profile_unchanged');
  const duplicate=fixture();fs.appendFileSync(duplicate.f,'- id: llm-pi-ai\n  config: {}\n');await rejects(()=>run(duplicate.input),/重复/,'duplicate_rows_rejected');
  const absent=fixture();fs.unlinkSync(absent.c);await run(absent.input);await run({...absent.input,action:'disconnect'});check(!fs.existsSync(absent.c),'missing_credential_file_restored_absent');
  const dead=fixture();fs.writeFileSync(path.join(dead.profile,'package.json.lock'),'2147483647\n');await run(dead.input);check((await run({...dead.input,action:'status'})).configured,'dead_writer_lock_recovered');await run({...dead.input,action:'disconnect'});
  const prezen=fixture();let pz=prezen.y.parse(fs.readFileSync(prezen.f,'utf8'));prezen.y.row(pz,'llm-zen-session-headers').getIn(['config','models']).add('pzen');fs.writeFileSync(prezen.f,String(pz));await run(prezen.input);pz=prezen.y.parse(fs.readFileSync(prezen.f,'utf8'));prezen.y.row(pz,'agent-default-model').setIn(['config','model'],'user2');fs.writeFileSync(prezen.f,String(pz));await run({...prezen.input,action:'disconnect'});check(prezen.y.row(prezen.y.parse(fs.readFileSync(prezen.f,'utf8')),'llm-zen-session-headers').getIn(['config','models']).toJSON().includes('pzen'),'preexisting_zen_rule_not_owned');
  const env=fixture();process.env.DSH_PATRICK_LOCAL_TOKEN='other';try{await rejects(()=>run(env.input),/环境变量覆盖/,'shadowing_env_rejected');}finally{delete process.env.DSH_PATRICK_LOCAL_TOKEN;}
  console.log(`DSH_BRIDGE PASS checks=${checks} formal_dsh_unchanged=YES requests=NONE exit=0`);
})().catch(e=>{console.error(e);process.exitCode=1;});
