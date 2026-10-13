// Runs in the installed DSH Node runtime. Only YAML is loaded from the profile;
// no DSH plugin is executed here. stdout is a bounded, secret-free status result.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { createRequire } = require('node:module');
const { execFileSync } = require('node:child_process');
const { isDeepStrictEqual: equal } = require('node:util');
const hash = text => crypto.createHash('sha256').update(text).digest('hex');
const fail = message => { throw new Error(message); };
const exists = p => fs.existsSync(p);
const read = p => fs.readFileSync(p, 'utf8');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
function alive(pid) { try { process.kill(pid, 0); return true; } catch(e) { return e.code !== 'ESRCH'; } }
function ordinary(p) {
  for (let cur = path.resolve(p); ; cur = path.dirname(cur)) {
    if (exists(cur) && fs.lstatSync(cur).isSymbolicLink()) fail('目标含目录链接，请使用实际 DSH profile 路径');
    if (path.dirname(cur) === cur) break;
  }
}
function privateDirectory(dir) {
  ordinary(dir);
  fs.mkdirSync(dir, {recursive:true, mode:0o700});
  if (process.platform === 'win32') {
    const sid = execFileSync('whoami.exe', ['/user','/fo','csv','/nh'], {windowsHide:true,encoding:'utf8'}).match(/S-1-5-[0-9-]+/)?.[0];
    if (!sid) fail('读取当前账户权限失败');
    execFileSync('icacls.exe', [dir,'/inheritance:r','/grant:r',`*${sid}:(OI)(CI)F`,'*S-1-5-18:(OI)(CI)F'], {windowsHide:true,stdio:'pipe'});
  }
}
async function atomic(filename, text, privateDir) {
  const temp = path.join(privateDir, crypto.randomBytes(12).toString('hex')+'.tmp');
  let fd;
  try {
    fd=fs.openSync(temp,'wx',0o600); fs.writeFileSync(fd,text); fs.fsyncSync(fd); fs.closeSync(fd); fd=undefined;
    for(let n=0;;n++) {
      try { fs.renameSync(temp,filename); break; }
      catch(e) { if(n>=8 || !['EPERM','EBUSY','EACCES'].includes(e.code))throw e; await sleep(100); }
    }
  } finally { if(fd!==undefined)fs.closeSync(fd); if(exists(temp))fs.unlinkSync(temp); }
}
// Same package.json.lock / credentials.yaml.lock protocol as dsh-atomic-write.
// Live or ambiguous locks are never stolen. DSH itself may reclaim dead locks.
async function lock(filename, fn) {
  const target=filename+'.lock'; const deadline=Date.now()+2500;
  for(;;) {
    try {fs.writeFileSync(target,process.pid+'\n',{flag:'wx',mode:0o600});break;}
    catch(e) {
      if(!['EEXIST','EPERM'].includes(e.code))throw e;
      ordinary(target);
      let record;try {record=read(target);}catch{}
      if(record && /^\d+\n$/.test(record) && Number(record)>0 && Number(record)<=0x7fffffff && !alive(Number(record))) {
        const claim=target+'.takeover-'+hash(record).slice(0,16);
        let claimed=false;
        try {fs.writeFileSync(claim,process.pid+'\n',{flag:'wx',mode:0o600});claimed=true;}catch{}
        if(claimed)try {if(exists(target)&&read(target)===record&&!alive(Number(record)))fs.unlinkSync(target);}finally{fs.unlinkSync(claim);}
      }
      if(Date.now()>deadline)fail('DSH 配置写锁繁忙，请稍后重试');await sleep(80);
    }
  }
  try {return await fn();} finally {fs.unlinkSync(target);}
}
function yamlTools(profile) {
  let y;
  try { y=createRequire(path.join(profile,'package.json'))('yaml'); } catch { fail('DSH profile 缺少 YAML 依赖，请先完成 DSH 安装'); }
  const parse = text => {
    const doc=y.parseDocument(text,{customTags:[{tag:'tag:yaml.org,2002:js',resolve:v=>v}]});
    if(doc.errors.length)fail('DSH YAML 解析失败，原文件已保留');
    return doc;
  };
  function row(doc,id) {
    const found=[];
    function walk(n) {
      if(y.isSeq(n))n.items.forEach(walk);
      else if(y.isMap(n)) {
        if(n.get('id')===id)found.push(n);
        // Plugin config and model IDs are not Loader entry IDs.
        for(const p of n.items)if(p.key.value!=='config')walk(p.value);
      }
    }
    walk(doc.contents);
    if(found.length>1)fail('DSH 存在重复插件配置项，请先合并后接入');
    return found[0];
  }
  function providers(doc) {
    const llm=row(doc,'llm-pi-ai');
    if(!llm || llm.get('disabled')===true)fail('未找到启用的 llm-pi-ai 配置');
    const p=llm.getIn(['config','providers']);
    if(!y.isMap(p))fail('DSH providers 不是可编辑的映射');
    return p;
  }
  return {...y,parse,row,providers};
}
function plain(node) {return node?.toJSON ? node.toJSON() : node;}
function matches(value, entries) {return (entries??[]).some(s=>typeof s==='string'&&(s.endsWith('*')?value.toUpperCase().startsWith(s.slice(0,-1).toUpperCase()):s.toUpperCase()===value.toUpperCase()));}
function generate(y,doc,input,ref) {
  const existing=plain(y.providers(doc)); const managed={};const zenIds=[];
  const zenRow=y.row(doc,'llm-zen-session-headers');const zen=plain(zenRow?.get('config'));
  if(zenRow && zen?.models!==undefined && !Array.isArray(zen.models))fail('Zen models 不是列表');
  const api={openai:'openai-completions',responses:'openai-responses',anthropic:'anthropic-messages'};
  for(const model of input.models) {
    if(!/^[a-zA-Z0-9_-]{1,100}$/.test(model.id)||!api[model.protocol])fail('本机模型 ID 或协议无效');
    const definitions=Object.entries(existing).flatMap(([route,p])=>(p.models??[]).filter(m=>m.id===model.model).map(m=>({route,p,m})));
    // Reuse only unambiguous declared capabilities; never copy upstream URLs or keys.
    const definition=definitions.length===1?definitions[0]:undefined;
    const local={id:model.id,name:model.name};
    if(definition && (!definition.p.api || definition.p.api===api[model.protocol])) {
      for(const key of ['input','reasoningEfforts','compat'])if(definition.m[key]!==undefined)local[key]=definition.m[key];
      for(const key of ['contextWindow','maxTokens'])if(definition.m[key]!==undefined)local[key]=definition.m[key];
    }
    if(model.contextWindow!=null)local.contextWindow=model.contextWindow;
    if(model.maxOutput!=null)local.maxTokens=model.maxOutput;
    const route='dsh-patrick-'+model.protocol;
    managed[route]??={displayName:'DSH Patrick · '+model.protocol,api:api[model.protocol],baseURL:`http://127.0.0.1:${input.port}`+(model.protocol==='anthropic'?'':'/v1'),apiKeyEnv:ref,headers:{'x-dsh-patrick':'1'},models:[]};
    managed[route].models.push(local);
    if(zenRow && zenRow.get('disabled')!==true && zen?.enabled!==false && (matches(model.model,zen?.models)||definitions.some(d=>matches(d.route,zen?.providers))))zenIds.push(model.id);
  }
  return {providers:managed,zenIds};
}
function removeOwned(y,current,journal) {
  const result={...current};
  for(const kind of ['profile','credentials']) {
    const value=current[kind];
    if(value===journal.after[kind] || value===journal.before[kind]) { result[kind]=journal.before[kind]; continue; }
    if(journal.phase==='pending' && journal.previous && value===journal.previous.after[kind]) {result[kind]=journal.before[kind];continue;}
    const doc=y.parse(value); const expected=y.parse(journal.after[kind]);
    if(kind==='profile') {
      const p=y.providers(doc),ep=y.providers(expected);
      for(const name of journal.routes) {
        const prior=journal.previous ? y.providers(y.parse(journal.previous.after.profile)).get(name) : undefined;
        if(p.has(name) && !equal(plain(p.get(name)),plain(ep.get(name))) && !equal(plain(p.get(name)),plain(prior)))fail('Patrick 模型入口被外部修改，已保留现场；请核对后重试恢复');
        p.delete(name);
      }
      const row=y.row(doc,'llm-zen-session-headers');
      if(journal.zenIds.length && row) {
        const list=row.getIn(['config','models']);
        if(!y.isSeq(list))fail('Zen 规则被外部改写，请核对后重试恢复');
        list.items=list.items.filter(n=>!journal.zenIds.includes(plain(n)));
      }
    } else {
      const v=doc.getIn(['refs',journal.ref]);
      if(v!==undefined && v!==expected.getIn(['refs',journal.ref]))fail('Patrick 本机令牌被外部修改，已保留现场');
      doc.deleteIn(['refs',journal.ref]);
    }
    result[kind]=String(doc);
  }
  return result;
}
async function run(input) {
  if(!['inspect','sync','disconnect','status'].includes(input.action))fail('未知接入操作');
  const profile=path.resolve(input.profile);
  if(!path.isAbsolute(input.profile)||path.basename(path.dirname(profile)).toLowerCase()!=='profiles')fail('请选择 DSH_HOME 下 profiles 中的 profile 目录');
  ordinary(profile);
  if(!exists(path.join(profile,'package.json')))fail('DSH profile 不存在或缺少 package.json');
  const home=path.dirname(path.dirname(profile));const file=path.join(profile,'cordis.patch.yml');
  const credentials=path.join(home,'.credentials.yaml');const directory=path.join(profile,'.dsh-patrick');const stateFile=path.join(directory,'journal.json');
  ordinary(file);ordinary(credentials);ordinary(directory);ordinary(stateFile);
  const y=yamlTools(profile);
  function inspect() {
    const doc=y.parse(read(file));y.providers(doc);
    if(exists(path.join(home,'cordis.patch.yml'))) {
      const overlay=y.parse(read(path.join(home,'cordis.patch.yml')));
      if(y.row(overlay,'llm-pi-ai')||y.row(overlay,'llm-zen-session-headers'))fail('DSH home overlay 覆盖模型配置，请先核对接入层级');
    }
    if(exists(credentials)) {
      const c=y.parse(read(credentials));
      if(!y.isMap(c.contents)||c.get('version')!==1||!y.isMap(c.get('refs')))fail('DSH 凭据文件格式不受支持，原文件已保留');
    }
    return {profile,configured:false,modelCount:0,zenCount:0,message:'已找到 DSH profile；尚未同步',backupDirectory:exists(directory)?directory:''};
  }
  const status=inspect();
  if(input.action==='inspect')return status;
  if(input.action==='status') {
    if(!exists(stateFile))return status;
    const j=JSON.parse(read(stateFile));
    if(j.owner!==input.owner)fail('此 DSH profile 已由另一份 Patrick 配置接入');
    if(j.phase!=='active')return {...status,message:'上次接入中断，请点击断开并恢复'};
    const current={profile:read(file),credentials:read(credentials)};
    removeOwned(y,current,j); // Conflict check without writes.
    const d=y.parse(current.profile), c=y.parse(current.credentials), e=y.parse(j.after.profile);
    const rules=plain(y.row(d,'llm-zen-session-headers')?.getIn(['config','models']))??[];
    const configured=j.routes.every(r=>equal(plain(y.providers(d).get(r)),plain(y.providers(e).get(r)))) && j.zenIds.every(id=>rules.includes(id)) && c.getIn(['refs',j.ref])===y.parse(j.after.credentials).getIn(['refs',j.ref]);
    return {...status,configured,modelCount:j.modelCount,zenCount:j.zenIds.length,message:configured?'配置已同步；DSH 是否加载以客户端模型列表及请求记录为准':'配置已变化，请重新同步',backupDirectory:directory};
  }
  privateDirectory(directory);
  return lock(path.join(profile,'package.json'),()=>lock(credentials,async()=>{
    let old=exists(stateFile)?JSON.parse(read(stateFile)):null;
    if(old && old.owner!==input.owner)fail('此 DSH profile 已由另一份 Patrick 配置接入，请先从原软件断开');
    if(old && old.pid!==input.pid && alive(old.pid))fail('另一 Patrick 实例正在使用此 DSH profile');
    const beforeRead={profile:read(file),credentials:exists(credentials)?read(credentials):'version: 1\nrefs: {}\n'};
    const base=old?removeOwned(y,beforeRead,old):beforeRead;
    if(input.action==='disconnect') {
      if(!old)return {...status,message:'未发现需要恢复的 Patrick 接入'};
      // Detach routes first, then revoke the loopback credential.
      await atomic(file,base.profile,directory);
      if(old.credentialsExisted===false && base.credentials==='version: 1\nrefs: {}\n') {if(exists(credentials))fs.unlinkSync(credentials);}
      else await atomic(credentials,base.credentials,directory);
      fs.unlinkSync(stateFile);
      return {...status,message:'已撤销 Patrick 模型入口，原有模型和默认选择保持不变',backupDirectory:directory};
    }
    if(!Number.isInteger(input.port)||input.port<1024||input.port>65535||!/^patrick_[a-f0-9]{64}$/.test(input.token)||!Array.isArray(input.models)||input.models.length>100)fail('同步输入不完整');
    const doc=y.parse(base.profile),c=y.parse(base.credentials);const ref='DSH_PATRICK_LOCAL_TOKEN';
    if(process.env[ref] && process.env[ref]!==input.token)fail('环境变量覆盖 Patrick 本机令牌，请先清理同名环境变量');
    const generated=generate(y,doc,input,ref);
    const p=y.providers(doc);
    for(const name of Object.keys(generated.providers))if(p.has(name))fail('DSH 中已有同名 Patrick provider，原配置已保留');
    if(c.hasIn(['refs',ref]))fail('DSH 中已有同名本机令牌引用，原凭据已保留');
    for(const [name,v] of Object.entries(generated.providers))p.set(name,doc.createNode(v));
    c.setIn(['refs',ref],input.token);
    if(generated.zenIds.length) {
      const z=y.row(doc,'llm-zen-session-headers');
      if(!z.hasIn(['config','models']))z.setIn(['config','models'],doc.createNode([]));
      const existingRules=plain(z.getIn(['config','models']));
      generated.zenIds=generated.zenIds.filter(id=>!existingRules.includes(id));
      for(const id of generated.zenIds)z.getIn(['config','models']).add(id);
    }
    const after={profile:String(doc),credentials:String(c)};
    const journal={version:1,phase:'pending',owner:input.owner,pid:input.pid,ref,credentialsExisted:old?.credentialsExisted??exists(credentials),routes:[...new Set([...Object.keys(generated.providers),...(old?.routes??[])])],zenIds:[...new Set([...generated.zenIds,...(old?.zenIds??[])])],modelCount:input.models.length,before:base,after};
    if(!old) {
      const backup=path.join(directory,'backup-'+Date.now()+'-'+crypto.randomBytes(4).toString('hex'));fs.mkdirSync(backup,{mode:0o700});
      fs.writeFileSync(path.join(backup,'cordis.patch.yml'),base.profile,{flag:'wx',mode:0o600});
      fs.writeFileSync(path.join(backup,'.credentials.yaml'),base.credentials,{flag:'wx',mode:0o600});
    }
    // Journal is durable before either file changes. Next sync/disconnect can recover partial commits.
    // It records both the previous and new ownership states for interrupted resynchronization.
    journal.previous=old;
    await atomic(stateFile,JSON.stringify(journal),directory);
    try {
      if(read(file)!==beforeRead.profile || (exists(credentials)?read(credentials):'version: 1\nrefs: {}\n')!==beforeRead.credentials)fail('DSH 文件在同步期间发生变化，请重试');
      await atomic(credentials,after.credentials,directory);await atomic(file,after.profile,directory);
      journal.phase='active';journal.routes=Object.keys(generated.providers);journal.zenIds=generated.zenIds;delete journal.previous;
      await atomic(stateFile,JSON.stringify(journal),directory);
    } catch(e) {
      // Roll back only bytes still owned by this transaction, never an intervening edit.
      if(read(file)===after.profile)await atomic(file,beforeRead.profile,directory);
      if(exists(credentials)&&read(credentials)===after.credentials)await atomic(credentials,beforeRead.credentials,directory);
      if(read(file)===beforeRead.profile && (!exists(credentials)||read(credentials)===beforeRead.credentials)) {
        if(old)await atomic(stateFile,JSON.stringify(old),directory);
        else {if(!journal.credentialsExisted&&exists(credentials))fs.unlinkSync(credentials);fs.unlinkSync(stateFile);}
      }
      throw e;
    }
    return {...status,configured:true,modelCount:input.models.length,zenCount:generated.zenIds.length,message:'配置已同步；请在 DSH 选择 DSH Patrick 下的模型',backupDirectory:directory};
  }));
}
module.exports={run,generate,yamlTools,removeOwned,hash};
if(require.main===module) {
  let text='';process.stdin.setEncoding('utf8');process.stdin.on('data',s=>{text+=s;if(text.length>2*1024*1024)process.exit(2);});
  process.stdin.on('end',async()=>{
    try {const value=await run(JSON.parse(text));process.stdout.write(JSON.stringify({ok:true,value}));}
    catch(error) {const message=/[\u3400-\u9fff]/.test(error.message)?error.message:'DSH 接入文件操作失败；请检查路径、依赖和写入权限';process.stdout.write(JSON.stringify({ok:false,error:message}));process.exitCode=1;}
  });
}
