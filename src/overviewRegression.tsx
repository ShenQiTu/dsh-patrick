import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { api } from "./api";
import { Overview, RangeActions } from "./Overview";
import { calendar, calendarLayout, localDate, localInput, presetRange, type Range, type UsageRecord } from "./stats";

export async function runOverviewRegression() {
  const checks: string[] = [];
  const pause = () => new Promise(r => setTimeout(r, 80));
  const check = (ok: unknown, name: string) => { if (!ok) throw new Error("OVERVIEW: " + name); checks.push(name); };
  const until = async (f: () => boolean, name: string) => { for (let i=0;i<100;i++) { if(f()) return; await pause(); } throw new Error("OVERVIEW timeout: "+name); };
  document.querySelector<HTMLElement>('[data-nav="overview"]')!.click(); await pause();
  const main = document.querySelector<HTMLElement>('main')!, existing = main.querySelector<HTMLElement>('[data-overview]')!;
  existing.style.display='none';
  const host=document.createElement('div');host.style.cssText='display:flex;flex-direction:column;flex:1;min-height:0;min-width:0';main.appendChild(host);
  const root=createRoot(host), originalStatistics=api.statistics;
  const end=new Date(2026,9,14,10,12,34,567).getTime();
  const fixture:UsageRecord[]=[13,14].map(d=>({id:`synthetic-${d}`,startedAt:new Date(2026,9,d,d===13?20:9).getTime(),source:'gateway',state:'success',inputTokens:100,outputTokens:20,cacheReadTokens:40,cacheWriteTokens:0}));
  const pricing={input:null,output:null,cacheRead:null,cacheWrite:null};
  try {
    api.statistics=async()=>fixture;
    root.render(<Overview version={0} historyError={null} preset="custom" custom={presetRange('month',end)} pricing={pricing}/>);
    await until(()=>host.querySelector('[data-usage-calls]')?.textContent==='2','synthetic cross-day records');
    const populated=Array.from(host.querySelectorAll<HTMLElement>('.token-column')).filter(el=>el.title.includes('1 次调用'));
    check(populated.length===2 && populated.map(el=>localDate(Number(el.dataset.bucketStart))).join()==='2026-10-13,2026-10-14','overview_cross_day_columns_separate');
    check(populated.every(el=>el.title.includes('不含结束时刻') && Number(el.dataset.bucketEnd)>Number(el.dataset.bucketStart)),'overview_tooltip_has_exact_interval');
    const savedTheme=document.documentElement.dataset.theme;
    for(const theme of ['light','dark']) {
      document.documentElement.dataset.theme=theme;
      for(const [width,height] of [[960,640],[820,550],[800,520]]) {
        await invoke('smoke_resize',{width,height});await until(()=>innerWidth===width && innerHeight===height,'resize');await pause();
        const tag=`${width}_${height}_${theme}`;
        const bars=host.querySelectorAll<HTMLElement>('.token-column');
        for(const tick of host.querySelectorAll<HTMLElement>('[data-axis-index]')) {
          const b=bars[Number(tick.dataset.axisIndex)].getBoundingClientRect(),t=tick.getBoundingClientRect();
          check(Math.abs(b.left-t.left)<1 && Math.abs(b.width-t.width)<1,'overview_tick_matches_column_'+tag+'_'+tick.dataset.axisIndex);
        }
        const labels=Array.from(host.querySelectorAll<HTMLElement>('.token-axis b')).map(el=>el.getBoundingClientRect());
        check(labels.every((r,i)=>i===0||labels[i-1].right<=r.left),'overview_axis_labels_no_overlap_'+tag);
        const days=calendar(fixture,end,'gateway'),layout=calendarLayout(days);
        const cells=Array.from(host.querySelectorAll<HTMLElement>('.calendar-squares > *'));
        for(const month of layout.months) {
          const label=host.querySelector<HTMLElement>(`[data-month-column="${month.column}"]`)!;
          check(Math.abs(label.getBoundingClientRect().left-cells[month.column*7].getBoundingClientRect().left)<1,'overview_month_aligned_'+tag+'_'+month.column);
        }
        const months=Array.from(host.querySelectorAll<HTMLElement>('.calendar-axis span')).map(el=>el.getBoundingClientRect());
        check(months.every((r,i)=>i===0||months[i-1].right<=r.left+1),'overview_month_labels_no_overlap_'+tag);
        check(main.scrollWidth<=main.clientWidth+1 && main.scrollHeight<=main.clientHeight+1,'overview_fixed_chart_no_overflow_'+tag);
      }
    }
    document.documentElement.dataset.theme=savedTheme;
    await invoke('smoke_resize',{width:960,height:640});await until(()=>innerWidth===960 && innerHeight===640,'restore size');
    const captured:{value:Range|null}={value:null};
    const stale={start:end-100*86400000,end:end-99*86400000};
    root.render(<RangeActions value="month" onSelect={()=>{}} custom={stale} onCustom={r=>{captured.value=r;}}/>);await pause();
    const before=Date.now();
    Array.from(host.querySelectorAll<HTMLElement>('button')).find(b=>b.textContent==='自定义')!.click();await pause();
    const from=document.querySelector<HTMLInputElement>('[aria-label="统计开始时间"]')!, to=document.querySelector<HTMLInputElement>('[aria-label="统计结束时间"]')!;
    const enteredEnd=new Date(to.value).getTime();
    check(enteredEnd>=before && enteredEnd<=Date.now() && new Date(from.value).getTime()===presetRange('month',enteredEnd).start,'overview_custom_uses_selected_month_not_stale_range');
    (document.querySelector('form.custom-range') as HTMLFormElement).requestSubmit();await pause();
    check(captured.value?.end===enteredEnd,'overview_custom_open_apply_preserves_end');
    const precise={start:end-60000,end};
    root.render(<RangeActions value="custom" onSelect={()=>{}} custom={precise} onCustom={r=>{captured.value=r;}}/>);await pause();
    Array.from(host.querySelectorAll<HTMLElement>('button')).find(b=>b.textContent==='自定义')!.click();await pause();
    check(document.querySelector<HTMLInputElement>('[aria-label="统计结束时间"]')!.value===localInput(end),'overview_custom_milliseconds_visible');
    (document.querySelector('form.custom-range') as HTMLFormElement).requestSubmit();await pause();
    check(captured.value?.start===precise.start && captured.value?.end===precise.end,'overview_custom_milliseconds_preserved');
  } finally {
    api.statistics=originalStatistics;root.unmount();host.remove();existing.style.display='';
  }
  return checks;
}
