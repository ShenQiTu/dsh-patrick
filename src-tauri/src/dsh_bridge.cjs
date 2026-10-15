// Runs with Patrick's bundled runtime and YAML parser, never target DSH modules.
// No DSH plugin is executed here. stdout is a bounded, secret-free status result.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const YAML = require('yaml');
const { execFileSync } = require('node:child_process');
const { isDeepStrictEqual: equal } = require('node:util');
const hash = text => crypto.createHash('sha256').update(text).digest('hex');
const fail = message => { throw Object.assign(new Error(message),{publicMessage:message}); };
const publicMessage = error => error.publicMessage || 'DSH 接入文件操作失败；请检查所填 profile 路径和文件权限';
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
function yamlTools() {
  const y=YAML;
  const parse = (text,label='DSH YAML') => {
    const lineCounter=new y.LineCounter();
    const doc=y.parseDocument(text,{lineCounter,customTags:[{tag:'tag:yaml.org,2002:js',resolve:v=>v}]});
    if(doc.errors.length) {
      const pos=lineCounter.linePos(doc.errors[0].pos[0]);
      fail(`${label} 解析失败（第 ${pos.line} 行，第 ${pos.col} 列）；请检查缩进、冒号及括号，原文件已保留`);
    }
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
    if(found.length>1)fail(`DSH 存在重复插件配置项 ${id}；请核对该项后接入`);
    return found[0];
  }
  function providers(doc) {
    const llm=row(doc,'llm-pi-ai');
    if(!llm)fail('profile/cordis.patch.yml 未找到 llm-pi-ai 配置；请确认所填 profile 是实际使用的模型配置目录');
    if(llm.get('disabled')===true)fail('profile/cordis.patch.yml 中 llm-pi-ai 已禁用；请先在 DSH 核对模型配置');
    const p=llm.getIn(['config','providers']);
    if(!y.isMap(p))fail('profile/cordis.patch.yml 的 llm-pi-ai.config.providers 不是可编辑的映射；请核对该项结构');
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
// Detection is read-only and records the first failed prerequisite; later rows remain unchecked.
const checkDefinitions=[['path','配置目录'],['manifest','package.json'],['yaml','cordis.patch.yml 语法'],['providers','模型配置结构'],['overlay','上层覆盖配置'],['credentials','凭据文件结构']];
function readChecked(file,label,kind='file') {
  let stat;
  try {stat=fs.statSync(file);} catch(error) {
    if(error.code==='ENOENT')fail(`${label} ${kind==='directory'?'不存在':'缺少'}；请核对所填配置目录`);
    if(['EACCES','EPERM'].includes(error.code))fail(`${label} 读取权限不足；请检查当前账户的访问权限`);
    fail(`${label} 读取失败；请检查路径与文件状态`);
  }
  if(kind==='directory') {if(!stat.isDirectory())fail('所填路径是文件而非目录；请选择 DSH profile 文件夹');return;}
  if(!stat.isFile())fail(`${label} 应为文件，但当前位置是目录或其他对象`);
  try {return read(file);} catch(error) {
    if(['EACCES','EPERM'].includes(error.code))fail(`${label} 读取权限不足；请检查当前账户的访问权限`);
    fail(`${label} 读取失败；请检查文件是否存在或被占用`);
  }
}
function optionalFile(file,label) {
  try {fs.statSync(file);return true;} catch(error) {
    if(error.code==='ENOENT')return false;
    if(['EACCES','EPERM'].includes(error.code))fail(`${label} 读取权限不足；请检查当前账户的访问权限`);
    fail(`${label} 状态读取失败；请检查文件路径与访问权限`);
  }
}
function readJournal(file) {
  let j;try {j=JSON.parse(readChecked(file,'.dsh-patrick/journal.json'));} catch(error) {
    if(error.publicMessage)throw error;
    fail('.dsh-patrick/journal.json 格式错误；请保留接入记录与备份，核对后再恢复');
  }
  if(!j || typeof j.owner!=='string' || !['active','pending'].includes(j.phase) || !Array.isArray(j.routes) || !Array.isArray(j.zenIds) || typeof j.ref!=='string'
      || !j.before || !j.after || ['profile','credentials'].some(k=>typeof j.before[k]!=='string'||typeof j.after[k]!=='string'))
    fail('.dsh-patrick/journal.json 格式不完整；请保留接入记录与备份，核对后再恢复');
  return j;
}
async function run(input) {
  const diagnostics={profile:typeof input.profile==='string'?input.profile:'',configured:false,modelCount:0,zenCount:0,message:'',backupDirectory:'',profileValid:false,checkedAt:Date.now(),syncState:'unchecked',checks:checkDefinitions.map(([id,label])=>({id,label,state:'unchecked',detail:'前置条件尚未通过，此项未检查'}))};
  try {
    const value=await runChecked(input,diagnostics);
    return {...diagnostics,...value,syncState:input.action==='inspect'?'unchecked':value.configured?'synced':value.message.includes('上次接入中断')?'error':'not_synced'};
  } catch(error) {
    diagnostics.message=publicMessage(error);
    diagnostics.syncState=diagnostics.profileValid?'error':'unchecked';
    error.diagnostics=diagnostics;
    throw error;
  }
}
async function runChecked(input,diagnostics) {
  if(!['inspect','sync','disconnect','status'].includes(input.action))fail('未知接入操作');
  function step(id,fn,detail) {
    const row=diagnostics.checks.find(c=>c.id===id);
    try {const value=fn();row.state='pass';row.detail=detail;return value;}
    catch(error) {row.state='fail';row.detail=publicMessage(error);throw error;}
  }
  const profile=step('path',()=>{
    if(typeof input.profile!=='string'||!input.profile.trim())fail('请先填写 DSH profile 目录并保存路径');
    if(!path.isAbsolute(input.profile))fail('DSH profile 目录须填写绝对路径，例如 DSH_HOME 下 profiles 中的实际 profile 目录');
    const p=path.resolve(input.profile);readChecked(p,'配置目录','directory');ordinary(p);
    if(path.basename(p).toLowerCase()==='profiles')fail('当前填的是 profiles 父目录；请选择其中实际使用的 profile 文件夹');
    if(path.basename(path.dirname(p)).toLowerCase()!=='profiles')fail('当前路径不是 profiles 下的 profile 目录；请填写配置目录，而非 DSH 安装目录');
    return p;
  },'目录存在，层级符合当前接入要求；这不代表运行中的 DSH 已加载它');
  step('manifest',()=>{ordinary(path.join(profile,'package.json'));readChecked(path.join(profile,'package.json'),'profile/package.json');},'必要文件存在且可读；不加载其依赖');
  const home=path.dirname(path.dirname(profile));const file=path.join(profile,'cordis.patch.yml');
  const credentials=path.join(home,'.credentials.yaml');const directory=path.join(profile,'.dsh-patrick');const stateFile=path.join(directory,'journal.json');
  const y=yamlTools();
  function inspect() {
    const doc=step('yaml',()=>{ordinary(file);return y.parse(readChecked(file,'profile/cordis.patch.yml'),'profile/cordis.patch.yml');},'YAML 语法检查通过');
    step('providers',()=>{
      y.providers(doc);
      const zen=y.row(doc,'llm-zen-session-headers');
      if(zen?.hasIn(['config','models'])&&!y.isSeq(zen.getIn(['config','models'])))fail('profile/cordis.patch.yml 的 llm-zen-session-headers.config.models 不是列表；请核对该项结构');
    },'已找到启用的 llm-pi-ai 及可编辑的 config.providers');
    step('overlay',()=>{
      const overlayFile=path.join(home,'cordis.patch.yml');ordinary(overlayFile);
      if(optionalFile(overlayFile,'DSH_HOME/cordis.patch.yml')) {
        const overlay=y.parse(readChecked(overlayFile,'DSH_HOME/cordis.patch.yml'),'DSH_HOME/cordis.patch.yml');
        if(y.row(overlay,'llm-pi-ai')||y.row(overlay,'llm-zen-session-headers'))fail('DSH_HOME/cordis.patch.yml 的 overlay 覆盖模型或 Zen 配置；请先核对实际接入层级，原文件已保留');
      }
    },'未发现上层模型或 Zen 配置覆盖冲突');
    let credentialsPresent=false;
    step('credentials',()=>{
      ordinary(credentials);credentialsPresent=optionalFile(credentials,'DSH_HOME/.credentials.yaml');
      if(credentialsPresent) {
        const c=y.parse(readChecked(credentials,'DSH_HOME/.credentials.yaml'),'DSH_HOME/.credentials.yaml');
        if(!y.isMap(c.contents)||c.get('version')!==1||!y.isMap(c.get('refs')))fail('DSH_HOME/.credentials.yaml 结构不匹配；应为 version: 1 且 refs 为映射，原文件已保留');
      }
    },'凭据结构检查通过；不显示凭据内容');
    if(!credentialsPresent)diagnostics.checks.find(c=>c.id==='credentials').detail='凭据文件尚不存在；同步时按原流程创建，本次检查未写入';
    diagnostics.profileValid=true;
    ordinary(directory);ordinary(stateFile);
    diagnostics.backupDirectory=exists(directory)?directory:'';
    return {profile,configured:false,modelCount:0,zenCount:0,message:'目录结构检查通过；配置尚未同步',backupDirectory:exists(directory)?directory:''};
  }
  const status=inspect();
  if(input.action==='inspect')return status;
  if(input.action==='status') {
    if(!optionalFile(stateFile,'.dsh-patrick/journal.json'))return status;
    const j=readJournal(stateFile);
    if(j.owner!==input.owner)fail('此 DSH profile 已由另一份 Patrick 配置接入');
    if(j.phase!=='active')return {...status,message:'上次接入中断，请点击断开并恢复'};
    const current={profile:readChecked(file,'profile/cordis.patch.yml'),credentials:readChecked(credentials,'DSH_HOME/.credentials.yaml')};
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
    catch(error) {const message=error.diagnostics?.message||publicMessage(error);process.stdout.write(JSON.stringify({ok:false,error:message,diagnostics:error.diagnostics}));process.exitCode=1;}
  });
}
