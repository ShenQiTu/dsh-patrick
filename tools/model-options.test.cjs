const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {run,yamlTools,generate}=require(path.resolve(process.argv[2]||'src-tauri/src/dsh_bridge.bundle.cjs'));
const y=yamlTools(),base=path.resolve(process.env.DSH_PATRICK_TEST_ROOT||assert.fail('isolated test root required'));
assert(!fs.existsSync(base));fs.mkdirSync(base,{recursive:true});let checks=0,index=0;
const check=(ok,name)=>{assert(ok,name);checks++;console.log('PASS '+name);};
const presets=['preset-standard','preset-ptc','preset-cordis'];
const original=y.stringify([
  {id:'llm-pi-ai',config:{providers:{original:{api:'openai-completions',baseURL:'https://example.invalid/v1',apiKeyEnv:'SYNTHETIC',streamIdleTimeoutMs:123456,models:[{id:'vision-model',input:['text','image']}]}}}},
  ...presets.map(id=>({id,config:{plugins:[{id:'compaction',name:'cordis:group',config:[{id:'compaction-basic',name:'@deepseek-ai/dsh-compaction-effort-inherit',config:{thresholdRatio:0.85,modelPolicies:[{provider:'original',model:'vision-model',thresholdRatio:0.8}]}}]}]}})),
  {id:'agent-default-model',config:{model:'untouched'}}
]);
const creds='version: 1\nrefs: {SYNTHETIC: SYNTHETIC_ONLY}\n';
function fixture(text=original) {
 const home=path.join(base,'case-'+(++index)),profile=path.join(home,'profiles/web');fs.mkdirSync(profile,{recursive:true});
 const f=path.join(profile,'cordis.patch.yml'),c=path.join(home,'.credentials.yaml'),j=path.join(profile,'.dsh-patrick/journal.json');
 fs.writeFileSync(path.join(profile,'package.json'),'{}');fs.writeFileSync(f,text);fs.writeFileSync(c,creds);
 return {home,f,c,j,text,input:{action:'sync',profile,owner:'synthetic-owner',pid:process.pid,port:48787,token:'patrick_'+'a'.repeat(64),models:[{id:'local',name:'Synthetic',model:'vision-model',protocol:'openai',compactionPercent:85,multimodal:true}]}};
}
const doc=t=>y.parse(fs.readFileSync(t.f,'utf8'));
const engine=(d,id)=>y.row(d,id).getIn(['config','plugins']).items[0].get('config').items[0];
const policies=(d,id)=>engine(d,id).getIn(['config','modelPolicies']).toJSON();
async function restore(t){await run({...t.input,action:'disconnect'});check(fs.readFileSync(t.f,'utf8')===t.text&&fs.readFileSync(t.c,'utf8')===creds&&!fs.existsSync(t.j),'exact_restore');}
(async()=>{
 const t=fixture(),generated=generate(y,doc(t),t.input,'SYNTHETIC');
 check(generated.providers['dsh-patrick-openai'].streamIdleTimeoutMs===300000,'default_dsh_wait_300_seconds');
 await run(t.input);let d=doc(t);
 if(process.env.DSH_PATRICK_COMPACTION_CONFIG) {
  const {pathToFileURL}=require('node:url');const {resolveConfig,resolveTargetPolicy}=await import(pathToFileURL(process.env.DSH_PATRICK_COMPACTION_CONFIG));
  for(const id of presets) {
   const config=resolveConfig(engine(d,id).get('config').toJSON());
   check(resolveTargetPolicy(config,{provider:'dsh-patrick-openai',model:'local'}).thresholdRatio===0.85,'installed_DSH_policy_resolves_85_'+id);
   check(resolveTargetPolicy(config,{provider:'original',model:'vision-model'}).thresholdRatio===0.8,'installed_DSH_original_policy_unchanged_'+id);
  }
 }
 for(const id of presets){check(policies(d,id).length===2&&policies(d,id)[1].thresholdRatio===0.85,'exact_rule_85_'+id);check(engine(d,id).getIn(['config','thresholdRatio'])===0.85&&policies(d,id)[0].provider==='original','original_policies_preserved_'+id);}
 check(y.providers(d).get('original').get('streamIdleTimeoutMs')===123456,'direct_channel_timeout_unchanged');
 check(y.providers(d).get('dsh-patrick-openai').get('models').toJSON()[0].input.join()==='text,image','vision_enabled');
 const active=await run({...t.input,action:'status'});check(active.configured&&active.modelVision.local===true,'status_and_inherited_capability');
 await run(t.input);check(policies(doc(t),presets[0]).length===2,'resync_no_duplicates');
 await run({...t.input,dshStreamIdleSeconds:900,models:[{...t.input.models[0],compactionPercent:90,multimodal:false}]});d=doc(t);
 check(y.providers(d).get('dsh-patrick-openai').get('streamIdleTimeoutMs')===900000,'seconds_to_ms');
 check(y.providers(d).get('dsh-patrick-openai').get('models').toJSON()[0].input.join()==='text','vision_disabled');
 check(presets.every(id=>policies(d,id)[1].thresholdRatio===0.9),'resync_replaces_only_own_ratio');
 await run({...t.input,models:[]});check(presets.every(id=>policies(doc(t),id).length===1),'delete_model_removes_only_owned');await restore(t);
 const inherited=fixture();inherited.input.models[0].multimodal=null;inherited.input.models[0].compactionPercent=null;await run(inherited.input);
 check(y.providers(doc(inherited)).get('dsh-patrick-openai').get('models').toJSON()[0].input.includes('image'),'legacy_vision_inherited');
 check(policies(doc(inherited),presets[0]).length===1,'legacy_compaction_unmodified');await restore(inherited);
 const unrelated=fixture();await run(unrelated.input);d=doc(unrelated);engine(d,presets[0]).getIn(['config','modelPolicies']).add({provider:'new-user-provider',model:'new-user-model',thresholdRatio:0.7});y.row(d,'agent-default-model').setIn(['config','model'],'user-edit');fs.writeFileSync(unrelated.f,String(d));await run({...unrelated.input,action:'disconnect'});d=doc(unrelated);
 check(policies(d,presets[0]).length===2&&policies(d,presets[0])[1].provider==='new-user-provider'&&y.row(d,'agent-default-model').getIn(['config','model'])==='user-edit','unrelated_edits_survive_restore');
 const conflict=fixture();await run(conflict.input);d=doc(conflict);engine(d,presets[1]).getIn(['config','modelPolicies']).items[1].set('thresholdRatio',0.7);fs.writeFileSync(conflict.f,String(d));const before=fs.readFileSync(conflict.f,'utf8');
 await assert.rejects(()=>run({...conflict.input,action:'disconnect'}),/外部修改/);check(fs.readFileSync(conflict.f,'utf8')===before&&fs.existsSync(conflict.j),'owned_conflict_no_overwrite');
 const invalid=fixture();for(const value of [0,-1,101,NaN,Infinity,'85']){await assert.rejects(()=>run({...invalid.input,models:[{...invalid.input.models[0],compactionPercent:value}]}),/压缩阈值/);check(fs.readFileSync(invalid.f,'utf8')===original,'invalid_ratio_no_write_'+value);}
 await assert.rejects(()=>run({...invalid.input,models:[{...invalid.input.models[0],compactionPercent:16}]}),/保留比例/);check(fs.readFileSync(invalid.f,'utf8')===original,'native_retention_constraint_no_write');
 for(const value of [0,2147484,1.5]){await assert.rejects(()=>run({...invalid.input,dshStreamIdleSeconds:value}),/等待时间/);check(fs.readFileSync(invalid.f,'utf8')===original,'invalid_wait_no_write_'+value);}
 const absentDoc=y.parse(original);presets.forEach(id=>{absentDoc.contents.items=absentDoc.contents.items.filter(n=>n.get('id')!==id);});const absent=fixture(String(absentDoc));
 check((await run(absent.input)).message.includes('阈值尚未应用'),'missing_engine_explicit_warning');check((await run({...absent.input,action:'status'})).message.includes('阈值尚未应用'),'warning_survives_status');await restore(absent);
 const exactDoc=y.parse(original);for(const id of presets)engine(exactDoc,id).getIn(['config','modelPolicies']).add({provider:'dsh-patrick-openai',model:'local',thresholdRatio:0.85});
 const exact=fixture(String(exactDoc));await run(exact.input);d=doc(exact);y.row(d,'agent-default-model').setIn(['config','model'],'new-default');fs.writeFileSync(exact.f,String(d));await run({...exact.input,action:'disconnect'});check(presets.every(id=>policies(doc(exact),id).length===2),'preexisting_exact_policies_not_owned');
 const existing=fixture(String(exactDoc));existing.input.models[0].compactionPercent=90;await assert.rejects(()=>run(existing.input),/不同压缩规则/);check(fs.readFileSync(existing.f,'utf8')===existing.text,'preexisting_different_policy_preserved');
 const missingDoc=y.parse(original);for(const id of presets)engine(missingDoc,id).deleteIn(['config','modelPolicies']);const missingList=fixture(String(missingDoc));await run(missingList.input);d=doc(missingList);y.row(d,'agent-default-model').setIn(['config','model'],'later');fs.writeFileSync(missingList.f,String(d));await run({...missingList.input,action:'disconnect'});check(presets.every(id=>!engine(doc(missingList),id).hasIn(['config','modelPolicies'])),'absent_policy_field_restored_after_unrelated_edit');
 const overlay=fixture();fs.writeFileSync(path.join(overlay.home,'cordis.patch.yml'),'- id: preset-standard\n  config: {}\n');await assert.rejects(()=>run(overlay.input),/overlay/);check(fs.readFileSync(overlay.f,'utf8')===original,'shadowing_compaction_overlay_no_write');
 const pending=fixture();await run(pending.input);const old=JSON.parse(fs.readFileSync(pending.j));await run({...pending.input,models:[{...pending.input.models[0],compactionPercent:90}]});let next=JSON.parse(fs.readFileSync(pending.j));next.phase='pending';next.previous=old;next.compaction=old.compaction.map((v,i)=>({key:v.key,rules:[...v.rules,...next.compaction[i].rules]}));fs.writeFileSync(pending.j,JSON.stringify(next));d=y.parse(old.after.profile);y.row(d,'agent-default-model').setIn(['config','model'],'later');fs.writeFileSync(pending.f,String(d));await run({...pending.input,action:'disconnect'});d=doc(pending);check(presets.every(id=>policies(d,id).length===1)&&y.row(d,'agent-default-model').getIn(['config','model'])==='later','pending_resync_restores_old_ownership_preserves_external');
 const fail=fixture(),rename=fs.renameSync;fs.renameSync=(a,b)=>{if(b===fail.f)throw new Error('synthetic write failure');return rename(a,b);};try{await assert.rejects(()=>run(fail.input),/synthetic/);}finally{fs.renameSync=rename;}
 check(fs.readFileSync(fail.f,'utf8')===original&&fs.readFileSync(fail.c,'utf8')===creds&&!fs.existsSync(fail.j),'partial_write_rolls_back');
 console.log(`MODEL_OPTIONS_PASS checks=${checks} profiles=SYNTHETIC formal_dsh=UNCHANGED requests=NONE`);
})().catch(e=>{console.error(e);process.exitCode=1;});
