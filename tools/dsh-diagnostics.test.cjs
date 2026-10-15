const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),cp=require('node:child_process');
const bundle=path.resolve(process.argv[2]);const {run}=require(bundle);
const root=process.env.DSH_PATRICK_TEST_ROOT;assert(root&&!fs.existsSync(root));fs.mkdirSync(root,{recursive:true});
let checks=0,n=0;const check=(v,label)=>{assert(v,label);checks++;console.log('PASS '+label);};
const original='- id: llm-pi-ai\n  config:\n    providers: {}\n';
function fixture(){const home=path.join(root,'case-'+(++n)),profile=path.join(home,'profiles','desktop');fs.mkdirSync(profile,{recursive:true});fs.writeFileSync(path.join(profile,'package.json'),'{}');const file=path.join(profile,'cordis.patch.yml');fs.writeFileSync(file,original);return {home,profile,file,input:{action:'status',profile,owner:'TEST_OWNER',pid:process.pid,port:48787,token:'patrick_'+'a'.repeat(64),models:[]}};}
function tree(dir){const out={};for(const e of fs.readdirSync(dir,{withFileTypes:true})){const p=path.join(dir,e.name);out[e.name]=e.isSymbolicLink()?'LINK':e.isDirectory()?tree(p):crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');}return out;}
async function failure(t,stage,regex){const before=tree(t.home);let error;try{await run(t.input);}catch(e){error=e;}assert(error,'expected failure');check(!!error.diagnostics,'structured_diagnostics_on_error');const d=error.diagnostics;check(d.checks.some(c=>c.id===stage&&c.state==='fail'&&regex.test(c.detail)),stage+'_specific_error');check(!d.configured&&d.modelCount===0&&d.checkedAt>0,'error_no_stale_synced_state');check(JSON.stringify(tree(t.home))===JSON.stringify(before),'failed_check_read_only');return d;}
(async()=>{
 const missing=fixture();fs.unlinkSync(missing.file);await failure(missing,'yaml',/cordis.patch.yml.*缺少/);
 const good=fixture(),before=tree(good.home);const ok=await run(good.input);check(ok.profileValid&&ok.checks.length===6&&ok.checks.every(c=>c.state==='pass'),'all_structure_checks_pass');check(!ok.configured&&ok.syncState==='not_synced','valid_does_not_claim_synced');check(JSON.stringify(tree(good.home))===JSON.stringify(before),'valid_check_read_only');
 const blank=fixture();blank.input.profile='';await failure(blank,'path',/填写/);
 const relative=fixture();relative.input.profile='profiles/desktop';await failure(relative,'path',/绝对路径/);
 const absent=fixture();absent.input.profile=path.join(absent.home,'profiles/missing');await failure(absent,'path',/不存在/);
 const filePath=fixture();filePath.input.profile=filePath.file;await failure(filePath,'path',/目录.*文件|文件.*目录/);
 const parent=fixture();parent.input.profile=path.dirname(parent.profile);await failure(parent,'path',/profiles.*父目录/);
 const install=fixture();install.input.profile=install.home;await failure(install,'path',/安装目录/);
 const manifest=fixture();fs.unlinkSync(path.join(manifest.profile,'package.json'));await failure(manifest,'manifest',/package.json.*缺少/);
 const permission=fixture();const stat=fs.statSync;fs.statSync=(p,...args)=>{if(p===permission.file){throw Object.assign(new Error('SECRET_MUST_NOT_ECHO'),{code:'EACCES'});}return stat(p,...args);};try{await failure(permission,'yaml',/权限/);}finally{fs.statSync=stat;}
 const malformed=fixture();fs.writeFileSync(malformed.file,'- id: llm-pi-ai\n  config: [SYNTHETIC_SECRET_NEVER_ECHO');const bad=await failure(malformed,'yaml',/cordis.patch.yml.*解析失败.*第 \d+ 行/);check(!JSON.stringify(bad).includes('SYNTHETIC_SECRET_NEVER_ECHO'),'yaml_error_redacts_source');
 const disabled=fixture();fs.writeFileSync(disabled.file,'- id: llm-pi-ai\n  disabled: true\n  config: {providers: {}}');await failure(disabled,'providers',/llm-pi-ai.*禁用/);
 const noPlugin=fixture();fs.writeFileSync(noPlugin.file,'[]');await failure(noPlugin,'providers',/llm-pi-ai.*缺少|未找到.*llm-pi-ai/);
 const providers=fixture();fs.writeFileSync(providers.file,'- id: llm-pi-ai\n  config: {providers: []}');await failure(providers,'providers',/config.providers.*映射/);
 const dup=fixture();fs.appendFileSync(dup.file,original);await failure(dup,'providers',/llm-pi-ai.*重复|重复.*llm-pi-ai/);
 const overlay=fixture();fs.writeFileSync(path.join(overlay.home,'cordis.patch.yml'),original);await failure(overlay,'overlay',/DSH_HOME.*overlay/);
 const credentials=fixture();fs.writeFileSync(path.join(credentials.home,'.credentials.yaml'),'version: 2\nrefs: {}');await failure(credentials,'credentials',/credentials.yaml.*version.*1/);
 const credentialYaml=fixture();fs.writeFileSync(path.join(credentialYaml.home,'.credentials.yaml'),'refs: [CREDENTIAL_SECRET');const cd=await failure(credentialYaml,'credentials',/credentials.yaml.*解析失败/);check(!JSON.stringify(cd).includes('CREDENTIAL_SECRET'),'credential_error_redacts_source');
 for(const [stage,name] of [['overlay','cordis.patch.yml'],['credentials','.credentials.yaml']]) {
  const t=fixture(),target=path.join(t.home,name),oldStat=fs.statSync;
  fs.statSync=(p,...args)=>{if(p===target)throw Object.assign(new Error('HIDDEN_SECRET'),{code:'EACCES'});return oldStat(p,...args);};
  try{await failure(t,stage,/权限/);}finally{fs.statSync=oldStat;}
 }
 const zen=fixture();fs.appendFileSync(zen.file,'- id: llm-zen-session-headers\n  config: {models: {bad: shape}}');await failure(zen,'providers',/llm-zen-session-headers.config.models.*列表/);
 const directoryFile=fixture();fs.unlinkSync(directoryFile.file);fs.mkdirSync(directoryFile.file);await failure(directoryFile,'yaml',/cordis.patch.yml.*应为文件/);
 const attached=fixture();await run({...attached.input,action:'sync'});let status=await run(attached.input);check(status.profileValid&&status.configured&&status.syncState==='synced','synced_separate_from_structure');const j=path.join(attached.profile,'.dsh-patrick/journal.json');let journal=JSON.parse(fs.readFileSync(j));journal.owner='OTHER';fs.writeFileSync(j,JSON.stringify(journal));const foreignBefore=tree(attached.home);let foreign;try{await run(attached.input);}catch(e){foreign=e.diagnostics;}check(foreign.profileValid&&foreign.syncState==='error'&&!foreign.configured&&/另一份/.test(foreign.message),'ownership_failure_keeps_structure_pass');check(JSON.stringify(tree(attached.home))===JSON.stringify(foreignBefore),'foreign_status_read_only');check(foreign.backupDirectory===path.join(attached.profile,'.dsh-patrick'),'sync_error_preserves_backup_location');
 fs.writeFileSync(j,'{PRIVATE_JOURNAL_SECRET');let corrupt;try{await run(attached.input);}catch(e){corrupt=e.diagnostics;}check(corrupt.profileValid&&corrupt.syncState==='error'&&/journal.json.*格式/.test(corrupt.message)&&!JSON.stringify(corrupt).includes('PRIVATE_JOURNAL_SECRET'),'journal_error_specific_and_redacted');
 const cli=cp.spawnSync(process.execPath,[bundle],{input:JSON.stringify(malformed.input),encoding:'utf8'});const payload=JSON.parse(cli.stdout);check(cli.status===1&&!payload.ok&&payload.diagnostics.checks.some(c=>c.id==='yaml'&&c.state==='fail')&&!cli.stdout.includes('SYNTHETIC_SECRET_NEVER_ECHO'),'cli_failure_carries_structured_diagnostics');
 const reused=await run(good.input);check(reused.checks.every(c=>c.state==='pass')&&reused.profileValid,'checks_not_shared_between_requests');
 const preview=fixture();await run({...preview.input,action:'sync'});const restored=await run({...preview.input,action:'disconnect'});check(!restored.configured&&restored.syncState==='not_synced'&&fs.readFileSync(preview.file,'utf8')===original,'sync_restore_unchanged');
 console.log(`DIAGNOSTICS_PASS checks=${checks} detection=READ_ONLY errors=SPECIFIC secrets=NOT_ECHOED sync=SEPARATE requests=NONE`);
})().catch(e=>{console.error(e);process.exitCode=1;});
