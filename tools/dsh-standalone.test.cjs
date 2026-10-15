const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const cp=require('node:child_process');
const root=process.env.DSH_PATRICK_TEST_ROOT;assert(root&&!fs.existsSync(root));fs.mkdirSync(root,{recursive:true});
const bundle=path.join(__dirname,'../src-tauri/src/dsh_bridge.bundle.cjs');
const standalone=path.join(root,'patrick-bridge.cjs');fs.copyFileSync(bundle,standalone);
const {run,yamlTools}=require(standalone);
const original='# keep comment\n- id: llm-pi-ai\n  config:\n    providers:\n      old:\n        apiKeyEnv: ORIGINAL_TEST_KEY\n        models: [{id: untouched}]\n- id: default-model\n  config: {model: untouched}\n- id: extra\n  config:\n    value: !!js process.env.TEST_ONLY\n';
const credentials='version: 1\nrefs:\n  ORIGINAL_TEST_KEY: SYNTHETIC_NOT_REAL\n';
let checks=0;const check=(v,n)=>{assert(v,n);checks++;console.log('PASS '+n)};
function fixture(name){const home=path.join(root,name),profile=path.join(home,'profiles',name);fs.mkdirSync(profile,{recursive:true});fs.writeFileSync(path.join(profile,'package.json'),'{}');const f=path.join(profile,'cordis.patch.yml'),c=path.join(home,'.credentials.yaml');fs.writeFileSync(f,original);fs.writeFileSync(c,credentials);return {home,profile,f,c,request:{action:'sync',profile,owner:'SYNTHETIC_OWNER',pid:process.pid,port:48787,token:'patrick_'+'a'.repeat(64),models:[{id:'local_test',name:'test',model:'synthetic',protocol:'openai'}]}};}
(async()=>{
 for(const name of ['web','desktop','手动自定义']) {
  const t=fixture(name);check(!fs.existsSync(path.join(t.profile,'node_modules')),'no_profile_modules_'+name);
  const inspect=await run({...t.request,action:'inspect'});check(!inspect.configured&&!fs.existsSync(path.join(t.profile,'.dsh-patrick')),'inspect_read_only_'+name);
  check((await run(t.request)).configured,'sync_with_self_contained_parser_'+name);
  check((await run({...t.request,action:'status'})).modelCount===1,'status_verified_'+name);
  await run({...t.request,action:'disconnect'});
  check(fs.readFileSync(t.f,'utf8')===original&&fs.readFileSync(t.c,'utf8')===credentials,'exact_restore_'+name);
 }
 const poison=fixture('poisoned');const marker=path.join(poison.home,'TARGET_MODULE_EXECUTED');fs.mkdirSync(path.join(poison.profile,'node_modules/yaml'),{recursive:true});fs.writeFileSync(path.join(poison.profile,'node_modules/yaml/index.js'),`require('node:fs').writeFileSync(${JSON.stringify(marker)},'bad');throw new Error('loaded target code');`);
 check((await run(poison.request)).configured&&!fs.existsSync(marker),'target_yaml_module_never_loaded');await run({...poison.request,action:'disconnect'});
 const noGlobal=cp.spawnSync(process.execPath,[standalone],{encoding:'utf8',input:JSON.stringify({...poison.request,action:'inspect'}),env:{SystemRoot:process.env.SystemRoot,PATH:path.join(process.env.SystemRoot,'System32'),TEMP:process.env.TEMP,TMP:process.env.TMP,ProgramFiles:path.join(root,'no-system-node')}});
 check(noGlobal.status===0&&JSON.parse(noGlobal.stdout).ok,'standalone_bundle_no_node_path_or_target_parser');
 await assert.rejects(()=>run({...poison.request,action:'scan'}),/未知接入操作/);check(true,'scan_action_absent');
 const bad=fixture('bad');fs.writeFileSync(bad.f,'[');await assert.rejects(()=>run(bad.request),/解析失败/);check(fs.readFileSync(bad.f,'utf8')==='['&&!fs.existsSync(path.join(bad.profile,'.dsh-patrick')),'bad_yaml_unchanged');
 // A journal made by the previous bridge must remain recoverable after upgrade.
 const oldSource=process.env.DSH_PATRICK_OLD_SOURCE;assert(oldSource);const old=require(path.join(oldSource,'src-tauri/src/dsh_bridge.cjs'));const legacy=fixture('legacy');const shim=path.join(legacy.profile,'node_modules/yaml/index.js');fs.mkdirSync(path.dirname(shim),{recursive:true});fs.writeFileSync(shim,`module.exports=require(${JSON.stringify(require.resolve('yaml'))});`);await old.run(legacy.request);fs.unlinkSync(shim);
 check((await run({...legacy.request,action:'status'})).configured,'legacy_journal_compatible_without_old_parser');await run({...legacy.request,action:'disconnect'});check(fs.readFileSync(legacy.f,'utf8')===original&&fs.readFileSync(legacy.c,'utf8')===credentials,'legacy_journal_exact_restore');
 const text=fs.readFileSync(bundle,'utf8');check(!text.includes('dsh_scan')&&!text.includes('createRequire'),'cancelled_scanner_and_target_require_excluded');
 console.log(`STANDALONE_PASS checks=${checks} parser=BUNDLED target_modules=NOT_LOADED legacy_restore=EXACT scanner=NONE requests=NONE`);
})().catch(e=>{console.error(e);process.exitCode=1;});
