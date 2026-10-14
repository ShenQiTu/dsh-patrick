import assert from 'node:assert/strict';
import { summarize, estimateCost, presetRange, calendar, calendarLayout, localDate, localInput, parseLocalInput, customRange, timeBuckets, axisTicks, day } from '../src/stats.ts';
const now = new Date(2026, 9, 13, 12).getTime();
const call = (id, extra={}) => ({id,startedAt:now-1000,source:'gateway',state:'success',inputTokens:100,outputTokens:20,cacheReadTokens:40,cacheWriteTokens:0,...extra});
let checks=0;
function check(ok) { assert.ok(ok); checks++; }
const rows = [call('one'),call('two',{cacheReadTokens:null}),call('test',{source:'test'}),call('old',{startedAt:now-40*day}),call('error',{state:'error',inputTokens:null,outputTokens:null,cacheReadTokens:null})];
const s=summarize(rows,presetRange('hour',now));
check(s.selected.length===3); check(s.input===200 && s.output===40); check(s.cacheRate===0.4); check(s.cacheSamples===1); check(s.failed===1 && s.success===2);
check(s.buckets.reduce((n,b)=>n+b.input+b.output+b.read+b.write,0)===240);
check(summarize(rows,presetRange('hour',now),'test').selected.length===1);
check(summarize(rows,presetRange('hour',now),'all').selected.length===4);
check(summarize([],presetRange('hour',now)).cacheRate===null);
check(summarize([call('no-cache',{cacheReadTokens:null})],presetRange('hour',now)).cacheRate===null);
check(summarize([call('zero',{cacheReadTokens:0})],presetRange('hour',now)).cacheRate===0);
check(summarize([call('boundary',{startedAt:now})],presetRange('hour',now)).selected.length===0);
check(presetRange('today',now).start===new Date(2026,9,13).getTime());
check(new Date(presetRange('month',new Date(2026,2,31).getTime()).start).getDate()===28);
const days=calendar(rows,now,'gateway'); check(days.length===365); check(days.reduce((n,d)=>n+d.count,0)===4);
check(calendar(rows,now,'test').reduce((n,d)=>n+d.count,0)===1);
const pricing={input:5,output:25,cacheRead:0.5,cacheWrite:6.25};
const blank={input:null,output:null,cacheRead:null,cacheWrite:null};
const cost=estimateCost([call('price',{cacheWriteTokens:10})],pricing);
check(Math.abs(cost.amount-(50*5+20*25+40*0.5+10*6.25)/1e6)<1e-10 && cost.covered===1);
check(estimateCost([call('blank')],blank).amount===null);
check(estimateCost([call('free')],{input:0,output:0,cacheRead:0,cacheWrite:0}).amount===0);
check(estimateCost([call('missing')],{...pricing,cacheRead:null}).covered===0);
check(estimateCost([call('noWrite')],{...pricing,cacheWrite:null}).covered===1);
check(estimateCost([call('missingUsage',{outputTokens:null})],pricing).amount===null);
check(Math.abs(estimateCost([call('noCache',{cacheReadTokens:null})],pricing).amount-0.001)<1e-10);
check(estimateCost(s.selected,pricing).covered===2);
check(estimateCost(summarize(rows,presetRange('hour',now),'test').selected,pricing).covered===1);
check(estimateCost([],pricing).amount===null);
check(estimateCost([call('clamp',{cacheReadTokens:200,cacheWriteTokens:200})],pricing).amount===(100*0.5+20*25)/1e6);
const originalZone=process.env.TZ;
function partition(range,preset='custom') {
  const {buckets}=timeBuckets(range,preset);
  check(buckets.length>0 && buckets.length<=64);
  check(buckets[0].start===range.start && buckets.at(-1).end===range.end);
  for(let i=0;i<buckets.length;i++) {
    check(buckets[i].end>buckets[i].start);
    if(i)check(buckets[i-1].end===buckets[i].start);
  }
  const ticks=axisTicks(buckets.length);
  check(ticks.length<=5 && ticks[0]===0 && ticks.at(-1)===buckets.length-1 && new Set(ticks).size===ticks.length);
  const atEdges=buckets.flatMap((b,i)=>[call('start'+i,{startedAt:b.start}),call('last'+i,{startedAt:b.end-1})]);
  const s=summarize(atEdges,range,'gateway',preset);
  check(s.selected.length===atEdges.length && s.buckets.every(b=>b.calls===2));
  check(s.buckets.reduce((n,b)=>n+b.input+b.output+b.read+b.write,0)===s.input+s.output);
  return buckets;
}
for(const zone of ['Asia/Shanghai','UTC','America/Los_Angeles','Asia/Kolkata']) {
  process.env.TZ=zone;
  const end=new Date(2026,9,14,10,12,34,567).getTime();
  const records=[call('13',{startedAt:new Date(2026,9,13,20).getTime()}),call('14',{startedAt:new Date(2026,9,14,9).getTime()})];
  for(const preset of ['month','week']) {
    const range=presetRange(preset,end), summary=summarize(records,range,'gateway',preset);
    check(JSON.stringify(summary.buckets.filter(b=>b.calls).map(b=>[localDate(b.labelAt),b.calls]))===JSON.stringify([['2026-10-13',1],['2026-10-14',1]]));
    const bins=partition(range,preset);
    check(bins.every(b=>localDate(b.start)===localDate(b.end-1)));
    // Advancing the current time does not move completed days into another date.
    const later=summarize(records,presetRange(preset,end+3600000),'gateway',preset);
    check(later.buckets.filter(b=>b.calls).map(b=>localDate(b.labelAt)).join()===summary.buckets.filter(b=>b.calls).map(b=>localDate(b.labelAt)).join());
  }
  for(const preset of ['hour','ten','today']) partition(presetRange(preset,end),preset);
  check(timeBuckets(presetRange('today',end),'today').buckets.every(b=>new Date(b.labelAt).getMinutes()===0));
  const ten=timeBuckets(presetRange('ten',end),'ten').buckets;
  check(ten.length===11 && ten.every(b=>new Date(b.labelAt).getSeconds()===0));
  const midnight=new Date(2026,9,14).getTime();
  check(timeBuckets(presetRange('today',midnight),'today').buckets.length===0);
  check(timeBuckets(presetRange('today',midnight+1),'today').buckets.length===1);
  partition({start:midnight-5*60000,end:midnight+5*60000});
  const boundary=summarize([call('a',{startedAt:midnight-1}),call('b',{startedAt:midnight}),call('c',{startedAt:midnight+day})],{start:midnight-day,end:midnight+day},'gateway');
  check(boundary.selected.length===2 && boundary.buckets.length===2 && boundary.buckets.every(b=>b.calls===1));
  for(const [start,stop] of [
    [new Date(2026,11,31,22),new Date(2027,0,2,6)],
    [new Date(2024,1,28),new Date(2024,2,2)],
    [new Date(2026,1,28),new Date(2026,2,2)],
    [new Date(2025,0,17),new Date(2026,10,2)],
    [new Date(1900,0,1),new Date(2100,0,1)],
  ])partition({start:start.getTime(),end:stop.getTime()});
  const previousCustom={start:end-200*day,end:end-100*day};
  check(JSON.stringify(customRange('month',previousCustom,end))===JSON.stringify(presetRange('month',end)));
  check(customRange('custom',previousCustom,end)===previousCustom);
  check(parseLocalInput(localInput(end),end)===end && new Date(localInput(end)).getTime()===end);
  const before=JSON.stringify(records),days=calendar(records,end,'gateway'),layout=calendarLayout(days);
  check(days.length===365 && layout.columns>=53 && layout.columns<=54);
  check(days.filter(d=>d.count).map(d=>d.date).join()==='2026-10-13,2026-10-14');
  check(days.every((d,i)=>{const [y,m,day]=d.date.split('-').map(Number);return (i+layout.padding)%7===new Date(y,m-1,day).getDay();}));
  for(const month of layout.months) {
    check(days.some((d,i)=>Math.floor((i+layout.padding)/7)===month.column && `${Number(d.date.slice(5,7))}月`===month.label));
  }
  check(JSON.stringify(records)===before);
}
process.env.TZ='America/Los_Angeles';
const spring={start:new Date(2026,2,8).getTime(),end:new Date(2026,2,9).getTime()};
const fall={start:new Date(2026,10,1).getTime(),end:new Date(2026,10,2).getTime()};
check(partition(spring,'today').length===23);
check(partition(fall,'today').length===25);
check(partition({start:spring.start-day,end:spring.end+day},'week').length===3);
const repeated=new Date('2026-11-01T01:31:22.456-08:00').getTime();
check(parseLocalInput(localInput(repeated),repeated)===repeated);
check(timeBuckets({start:repeated,end:repeated+60000}).buckets[0].labelAt===repeated-22456);
for(const range of [{start:0,end:0},{start:1,end:0},{start:NaN,end:1},{start:0,end:Infinity}])check(timeBuckets(range).buckets.length===0);
check(axisTicks(0).length===0 && axisTicks(1).join()==='0');
if(originalZone===undefined)delete process.env.TZ;else process.env.TZ=originalZone;
console.log(`STATISTICS PASS checks=${checks} source_filter=OK cache_missing=UNKNOWN no_double_counting=OK calendar=LOCAL_TIME pricing=OK natural_days=OK axis=BUCKETS custom_precision=OK timezones=4 DST=OK`);
