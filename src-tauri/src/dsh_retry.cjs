// DSH owns retry scheduling. Patrick preserves this one declared provider option,
// not arbitrary provider fields, when checking, resynchronizing, or detaching.
const {isDeepStrictEqual:equal}=require('node:util');
const routes=new Set(['dsh-patrick-openai','dsh-patrick-responses','dsh-patrick-anthropic']);
const fail=field=>{throw Object.assign(new Error(`DSH retryPolicy.${field} 格式无效；原配置已保留`),{publicMessage:`DSH retryPolicy.${field} 格式无效；原配置已保留`});};
const object=v=>v!==null && typeof v==='object' && !Array.isArray(v);
function keys(value,allowed,field) {
  if(!object(value)||Object.keys(value).some(k=>!allowed.includes(k)))fail(field);
}
function validate(value) {
  if(value===undefined)return;
  keys(value,['mode','maxRetries','retryableCodes','backoff'],'配置项');
  if(!['normal','always'].includes(value.mode))fail('mode');
  if(value.mode==='normal') {
    if(value.maxRetries!==undefined && (!Number.isSafeInteger(value.maxRetries)||value.maxRetries<0))fail('maxRetries');
    if(value.retryableCodes!==undefined && (!Array.isArray(value.retryableCodes)||!value.retryableCodes.length||value.retryableCodes.some(c=>typeof c!=='string'||!c.length)||new Set(value.retryableCodes).size!==value.retryableCodes.length))fail('retryableCodes');
  }
  // DSH's always mode preserves but ignores normal-only fields after layered edits.
  if(value.backoff!==undefined) {
    const b=value.backoff;keys(b,['initialDelayMs','maxDelayMs','jitterRatio'],'backoff');
    const initial=b.initialDelayMs??500,max=b.maxDelayMs??10000,jitter=b.jitterRatio??0.1;
    if(!Number.isFinite(initial)||initial<=0||initial>2147483647||!Number.isFinite(max)||max<=0||max>2147483647||initial>max)fail('backoff.delay');
    if(!Number.isFinite(jitter)||jitter<0||jitter>1)fail('backoff.jitterRatio');
  }
}
function managed(value,check) {
  if(!object(value))return value;
  if(check)validate(value.retryPolicy);
  const result={...value};delete result.retryPolicy;return result;
}
function compatible(actual,expected) {return equal(managed(actual,true),managed(expected,false));}
function parseSaved(text) {
  if(text===null)return {};
  let data;try {data=JSON.parse(text);}catch{fail('保留记录');}
  keys(data,['version','providers'],'保留记录');
  if(data.version!==1||!object(data.providers)||Object.keys(data.providers).some(k=>!routes.has(k)))fail('保留记录');
  for(const policy of Object.values(data.providers))validate(policy);
  return data.providers;
}
function capture(providers,journal,saved) {
  const result={...saved};
  for(const route of journal?.routes??[]) {
    if(!routes.has(route))fail('渠道');
    const current=providers[route];
    if(current===undefined)continue;
    validate(current.retryPolicy);
    // An explicit removal uses DSH defaults and must not resurrect a cached policy.
    if(current.retryPolicy===undefined)delete result[route];
    else result[route]=current.retryPolicy;
  }
  return result;
}
function apply(providers,saved) {
  for(const [route,provider] of Object.entries(providers)) {
    if(saved[route]!==undefined) {validate(saved[route]);provider.retryPolicy=structuredClone(saved[route]);}
  }
}
module.exports={compatible,parseSaved,capture,apply};
