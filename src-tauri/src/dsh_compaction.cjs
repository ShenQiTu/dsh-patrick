// Exact per-provider/model overrides only; never change DSH global budgets.
const {isDeepStrictEqual:equal}=require('node:util');
const plain=n=>n?.toJSON?n.toJSON():n;
const fail=message=>{throw Object.assign(new Error(message),{publicMessage:message});};
const supported=new Set(['@deepseek-ai/dsh-compaction-basic','@deepseek-ai/dsh-compaction-effort-inherit']);
function engines(y,doc) {
  const found=new Map();
  for(const preset of ['preset-standard','preset-ptc','preset-cordis']) {
    const root=y.row(doc,preset);if(!root)continue;
    function walk(list,ids,active) {
      if(!y.isSeq(list))return;
      const seen=new Set();
      for(const row of list.items) {
        if(!y.isMap(row))continue;
        const id=row.get('id');if(typeof id!=='string')continue;
        if(seen.has(id))fail(`DSH ${ids.join('/')} 存在重复插件 ${id}`);seen.add(id);
        const path=[...ids,id], enabled=active&&row.get('disabled')!==true;
        if(supported.has(row.get('name')))found.set(JSON.stringify(path),{row,enabled});
        if(row.get('name')==='cordis:group')walk(row.get('config'),path,enabled);
      }
    }
    walk(root.getIn(['config','plugins']),[preset],root.get('disabled')!==true);
  }
  return found;
}
function apply(y,doc,models) {
  const requested=models.filter(m=>m.compactionPercent!=null),owned=[],expected=[];
  for(const m of requested)if(typeof m.compactionPercent!=='number'||!Number.isFinite(m.compactionPercent)||m.compactionPercent<=0||m.compactionPercent>100)fail('自动压缩阈值需大于 0 且不超过 100%');
  if(!requested.length)return {owned,expected,warning:''};
  const targets=[...engines(y,doc)].filter(([,v])=>v.enabled);
  for(const [key,{row}] of targets) {
    const config=row.get('config');if(!y.isMap(config))fail('DSH 压缩引擎配置不是映射；原文件已保留');
    const existing=plain(config.get('modelPolicies'))??[];
    if(!Array.isArray(existing))fail('DSH 压缩引擎 modelPolicies 不是列表');
    const rules=[],all=[];
    for(const m of requested) {
      const ratio=m.compactionPercent/100;
      const retain=config.get('retainTokens')==null?(config.get('retainRatio')??0.16):null;
      if(retain!=null&&ratio<=retain)fail(`模型 ${m.id} 的自动压缩阈值需高于 DSH 当前保留比例 ${retain*100}%`);
      const rule={provider:'dsh-patrick-'+m.protocol,model:m.id,thresholdRatio:ratio};
      all.push(rule);
      const prior=existing.filter(v=>v.provider===rule.provider&&v.model===rule.model);
      if(prior.length>1||prior.length===1&&!equal(prior[0],rule))fail(`DSH ${JSON.parse(key).join('/')} 已有该 Patrick 模型的不同压缩规则；原规则已保留`);
      if(!prior.length)rules.push(rule);
    }
    expected.push({key,rules:all});
    if(rules.length) {
      if(!config.has('modelPolicies'))config.set('modelPolicies',doc.createNode([]));
      for(const rule of rules)config.get('modelPolicies').add(doc.createNode(rule));
      owned.push({key,rules});
    }
  }
  return {owned,expected,warning:targets.length?'':'当前 profile 未找到启用的受支持压缩引擎；自动压缩阈值尚未应用，模型入口已同步'};
}
function remove(y,doc,journal) {
  const owned=journal.compaction??[];if(!owned.length)return;
  const targets=engines(y,doc),baseline=engines(y,y.parse(journal.before.profile));
  for(const {key,rules} of owned) {
    const row=targets.get(key)?.row;
    if(!row)fail('Patrick 压缩引擎入口被外部修改，请核对后恢复');
    const list=row.getIn(['config','modelPolicies']);
    if(list!==undefined&&!y.isSeq(list))fail('Patrick 压缩规则被外部修改，请核对后恢复');
    if(list) {
      for(const rule of rules) {
        const same=list.items.filter(n=>plain(n)?.provider===rule.provider&&plain(n)?.model===rule.model);
        if(same.length>1||same.some(n=>!rules.some(r=>equal(r,plain(n)))))fail('Patrick 压缩规则被外部修改，请核对后恢复');
      }
      list.items=list.items.filter(n=>!rules.some(r=>equal(r,plain(n))));
      if(!list.items.length&&!baseline.get(key)?.row.hasIn(['config','modelPolicies']))row.deleteIn(['config','modelPolicies']);
    }
  }
}
function present(y,doc,journal) {
  const targets=engines(y,doc);
  return (journal.compactionExpected??journal.compaction??[]).every(({key,rules})=>{
    const target=targets.get(key),list=plain(target?.row.getIn(['config','modelPolicies']));
    return target?.enabled&&Array.isArray(list)&&rules.every(r=>list.some(n=>equal(r,n)));
  });
}
function merge(a=[],b=[]) {
  const out=[];
  for(const {key,rules} of [...a,...b]) {
    let row=out.find(v=>v.key===key);if(!row){row={key,rules:[]};out.push(row);}
    for(const rule of rules)if(!row.rules.some(r=>equal(r,rule)))row.rules.push(rule);
  }
  return out;
}
module.exports={apply,remove,present,merge,engines};
