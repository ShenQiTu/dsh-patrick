// Optional installed-plugin contract probe: read-only import, synthetic profiles only.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {pathToFileURL}=require('node:url');
const {run,yamlTools}=require(path.resolve(process.argv[2]));
const base=process.env.DSH_PATRICK_TEST_ROOT;assert(base&&!fs.existsSync(base));fs.mkdirSync(base,{recursive:true});
const plugin=process.env.DSH_PATRICK_ZEN_PLUGIN;
const y=yamlTools();let count=0,n=0;
function check(value,name){assert(value,name);count++;console.log('PASS '+name);}
const initial={providers:['allZen'],routes:[{provider:'zen',models:['mimo-*']}]};
function fixture(policy=initial){
 const home=path.join(base,'case-'+(++n)),profile=path.join(home,'profiles/web');fs.mkdirSync(profile,{recursive:true});fs.writeFileSync(path.join(profile,'package.json'),'{}');
 const rows=[{id:'llm-pi-ai',config:{providers:{zen:{models:[{id:'mimo-free'}]},ordinary:{models:[{id:'normal'},{id:'shared'}]},allZen:{models:[{id:'other-zen'},{id:'shared'}]}}}},{id:'llm-zen-session-headers',config:policy},{id:'unrelated',config:{value:'keep'}}];
 const f=path.join(profile,'cordis.patch.yml'),c=path.join(home,'.credentials.yaml'),original=y.stringify(rows),credentials='version: 1\nrefs: {}\n';fs.writeFileSync(f,original);fs.writeFileSync(c,credentials);
 const input={action:'sync',profile,owner:'ZEN_SYNTHETIC_OWNER',pid:process.pid,port:48787,token:'patrick_'+'a'.repeat(64),models:[{id:'localZen',model:'mimo-free',name:'Zen',protocol:'openai'},{id:'localOther',model:'other-zen',name:'Other Zen',protocol:'responses'},{id:'localNormal',model:'normal',name:'Normal',protocol:'openai'},{id:'localShared',model:'shared',name:'Ambiguous',protocol:'openai'}]};
 return {home,profile,f,c,original,credentials,input,j:path.join(profile,'.dsh-patrick/journal.json')};
}
const config=t=>y.row(y.parse(fs.readFileSync(t.f,'utf8')),'llm-zen-session-headers').get('config').toJSON();
const eligible=(c,provider,model)=>c.providers?.includes(provider)||c.routes?.some(r=>r.provider===provider&&r.models.some(m=>m.endsWith('*')?model.startsWith(m.slice(0,-1)):m===model));
async function detach(t){await run({...t.input,action:'disconnect'});check(fs.readFileSync(t.f,'utf8')===t.original&&fs.readFileSync(t.c,'utf8')===t.credentials,'exact_restore');}
(async()=>{
 const t=fixture();await run(t.input);let c=config(t);
 console.log('ROUTE_ALIAS_MATCHED='+!!eligible(c,'dsh-patrick-openai','localZen'));
 console.log('LEGACY_GLOBAL_MODELS_ADDED='+!!c.models?.length);
 check(eligible(c,'dsh-patrick-openai','localZen')&&!c.models?.length,'new_plugin_routes_without_legacy_global_models');
 check(eligible(c,'dsh-patrick-responses','localOther'),'provider_rule_maps_only_declared_model');
 check(!eligible(c,'dsh-patrick-openai','localNormal')&&!eligible(c,'dsh-patrick-openai','localShared'),'normal_and_cross_provider_ambiguous_not_injected');
 check(!eligible(c,'unrelated','localZen')&&!c.providers.includes('dsh-patrick-openai'),'no_whole_provider_widening');
 const status=await run({...t.input,action:'status'});check(status.configured,'status_recognizes_owned_route_rules');check(status.message.includes('同名歧义'),'ambiguous_selection_reported_not_guessed');
 const first=fs.readFileSync(t.f,'utf8');await run(t.input);check(fs.readFileSync(t.f,'utf8')===first,'repeat_sync_idempotent');
 if(plugin){
  const p=await import(pathToFileURL(plugin).href);const sid='ses_123456789abc0123456789ABCD';
  for(const [provider,model,want] of [['dsh-patrick-openai','localZen',true],['dsh-patrick-responses','localOther',true],['dsh-patrick-openai','localNormal',false],['dsh-patrick-openai','localShared',false],['other','localZen',false]]){
   const out=p.applyZenSessionHeader({provider,model,sessionId:sid,requestHeaders:{'x-other':'keep'}},c);
   check((out.requestHeaders['x-opencode-session']===sid)===want&&(out.requestHeaders['x-opencode-session-id']===sid)===want,'installed_plugin_matches_'+model+'_'+provider);
   check(out.requestHeaders['x-other']==='keep','installed_plugin_keeps_other_headers');
  }
  const a=p.applyZenSessionHeader({provider:'dsh-patrick-openai',model:'localZen',sessionId:'fixture-session'},c,1791875767433).requestHeaders;
  check(a['x-opencode-session']==='ses_ecbe1a376ffe'+a['x-opencode-session'].slice(16)&&/^ses_[0-9a-f]{12}[0-9A-Za-z]{14}$/.test(a['x-opencode-session']),'installed_plugin_time_encoding_matches_validator');
 }
 await run({...t.input,models:[t.input.models[2]]});c=config(t);check(!eligible(c,'dsh-patrick-openai','localZen')&&!eligible(c,'dsh-patrick-responses','localOther'),'disable_delete_removes_only_owned_aliases');await detach(t);
 const external=fixture();await run(external.input);let d=y.parse(fs.readFileSync(external.f,'utf8'));y.row(d,'unrelated').setIn(['config','value'],'user-edit');y.row(d,'llm-zen-session-headers').getIn(['config','routes']).add(d.createNode({provider:'user-added',models:['keep']}));fs.writeFileSync(external.f,String(d));await run({...external.input,action:'disconnect'});c=config(external);check(eligible(c,'user-added','keep')&&eligible(c,'zen','mimo-free')&&!eligible(c,'dsh-patrick-openai','localZen'),'external_and_original_rules_retained');
 const pre=fixture({...initial,routes:[...initial.routes,{provider:'dsh-patrick-openai',models:['localZen']}]});await run(pre.input);d=y.parse(fs.readFileSync(pre.f,'utf8'));y.row(d,'unrelated').setIn(['config','value'],'changed');fs.writeFileSync(pre.f,String(d));await run({...pre.input,action:'disconnect'});check(eligible(config(pre),'dsh-patrick-openai','localZen'),'preexisting_alias_not_owned');
 const changed=fixture();await run(changed.input);d=y.parse(fs.readFileSync(changed.f,'utf8'));const list=y.row(d,'llm-zen-session-headers').getIn(['config','routes']);list.items.find(r=>r.get('provider')==='dsh-patrick-openai').get('models').add('external-change');fs.writeFileSync(changed.f,String(d));const before=fs.readFileSync(changed.f,'utf8');await assert.rejects(()=>run({...changed.input,action:'disconnect'}),/Zen.*外部|Zen.*改写/);check(fs.readFileSync(changed.f,'utf8')===before,'owned_rule_conflict_preserves_file');
 for(const policy of [{providers:['ZEN'],routes:[]},{providers:[],routes:[{provider:'zen',models:['MIMO-*']}]}]){const q=fixture(policy);await run(q.input);check(!eligible(config(q),'dsh-patrick-openai','localZen'),'case_sensitive_plugin_contract');await detach(q);}
 for(const policy of [{...initial,enabled:false},{providers:[],routes:[]}]){const q=fixture(policy);await run(q.input);check(!eligible(config(q),'dsh-patrick-openai','localZen'),'disabled_or_empty_not_injected');await detach(q);}
 for(const policy of [{routes:'bad'},{providers:['*'],routes:[]},{routes:[{provider:'zen',models:['*']}]}]){const q=fixture(policy);await assert.rejects(()=>run(q.input),/Zen/);check(fs.readFileSync(q.f,'utf8')===q.original&&!fs.existsSync(q.j),'invalid_rules_no_write');}
 const migrated=fixture({providers:[],models:['mimo-free']});await run(migrated.input);
 const legacyJournal=JSON.parse(fs.readFileSync(migrated.j));delete legacyJournal.zenRoutes;delete legacyJournal.zenAmbiguous;fs.writeFileSync(migrated.j,JSON.stringify(legacyJournal));
 d=y.parse(fs.readFileSync(migrated.f,'utf8'));y.row(d,'llm-zen-session-headers').set('config',d.createNode(initial));fs.writeFileSync(migrated.f,String(d));
 const migratedBytes=fs.readFileSync(migrated.f,'utf8');const migratedStatus=await run({...migrated.input,action:'status'});
 check(!migratedStatus.configured&&fs.readFileSync(migrated.f,'utf8')===migratedBytes,'legacy_journal_after_plugin_migration_status_read_only');
 await run(migrated.input);check(eligible(config(migrated),'dsh-patrick-openai','localZen')&&!config(migrated).models,'legacy_journal_resync_after_global_models_removed');
 await run({...migrated.input,action:'disconnect'});check(JSON.stringify(config(migrated))===JSON.stringify(initial)&&fs.readFileSync(migrated.c,'utf8')===migrated.credentials,'legacy_journal_disconnect_preserves_migrated_policy');
 const pending=fixture();await run(pending.input);const old=JSON.parse(fs.readFileSync(pending.j));await run({...pending.input,models:[pending.input.models[1]]});const newer=JSON.parse(fs.readFileSync(pending.j));newer.phase='pending';newer.previous=old;newer.routes=old.routes;newer.zenRoutes=[...newer.zenRoutes,...old.zenRoutes].filter((v,i,a)=>a.findIndex(x=>JSON.stringify(x)===JSON.stringify(v))===i);fs.writeFileSync(pending.j,JSON.stringify(newer));d=y.parse(old.after.profile);y.row(d,'unrelated').setIn(['config','value'],'external-during-crash');fs.writeFileSync(pending.f,String(d));await run({...pending.input,action:'disconnect'});check(!config(pending).routes.some(r=>r.provider.startsWith('dsh-patrick'))&&y.row(y.parse(fs.readFileSync(pending.f,'utf8')),'unrelated').getIn(['config','value'])==='external-during-crash','pending_scoped_resync_restore_retains_external_edit');
 const failwrite=fixture();const rename=fs.renameSync;fs.renameSync=(a,b)=>{if(b===failwrite.f)throw new Error('synthetic write failure');return rename(a,b);};try{await assert.rejects(()=>run(failwrite.input),/synthetic/);}finally{fs.renameSync=rename;}check(fs.readFileSync(failwrite.f,'utf8')===failwrite.original&&fs.readFileSync(failwrite.c,'utf8')===failwrite.credentials&&!fs.existsSync(failwrite.j),'scoped_partial_write_atomic_restore');
 console.log(`ZEN_COMPAT_PASS checks=${count} scoped_routes=YES ownership=EXACT plugin=${plugin?'INSTALLED_READ_ONLY':'NOT_LOADED'} requests=NONE`);
})().catch(e=>{console.error(e);process.exitCode=1;});
