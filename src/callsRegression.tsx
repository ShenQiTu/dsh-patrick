import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { api } from "./api";
import { Calls } from "./Calls";
import { emptyModel, type CallRecord } from "./types";

export async function runCallsRegression() {
  const checks: string[] = [];
  const wait = () => new Promise(r=>setTimeout(r,80));
  let scope:HTMLElement|null=null;
  const q = (s:string) => (scope?.querySelector<HTMLElement>(s) ?? document.querySelector<HTMLElement>(s))!;
  function check(ok:unknown,name:string) {if(!ok)throw new Error("CALLS: "+name);checks.push(name);}
  async function until(f:()=>boolean|Promise<boolean>,name:string) {for(let n=0;n<100;n++){if(await f())return;await wait();}throw new Error("CALLS timeout "+name);}
  async function click(s:string) {check(!!q(s),"exists_"+s);q(s).click();await wait();}
  async function select(label:string,text:string) {await click(`[role="combobox"][aria-label="${label}"]`);const option=Array.from(document.querySelectorAll<HTMLElement>('[role="option"]')).find(e=>e.textContent===text);check(option,"option_"+text);option!.click();await wait();}
  await click('[data-nav="settings"]');
  check(!!q('[data-settings-group="gateway"] [name="maxBodyMb"]'),"gateway_body_size_group");
  check(!!q('[data-settings-group="concurrency"] [name="bodyConcurrency"]') && !!q('[data-settings-group="concurrency"] [name="upstreamConcurrency"]'),"concurrency_separate_group");
  check(!!q('[data-settings-group="appearance"] [aria-label="界面主题"]'),"theme_separate_group");
  check(!!q('[data-settings-group="calls"] [aria-label="详细记录"]'),"calls_separate_group");
  await click('[aria-label="详细记录"]');
  (q('.settings-groups') as HTMLFormElement).requestSubmit();
  await until(async()=> (await api.snapshot()).settings.detailedLogging,"detail option saved");
  const state=await api.snapshot();
  const id=await api.save({...emptyModel(),name:"Calls detail fixture",model:"native-mock",baseUrl:state.testUpstream!,apiKey:"SYNTHETIC_CALLS_KEY"});
  const result=await api.test(id);check(result.state==="success","detailed_test_success");
  await click('[data-nav="calls"]');await click('[aria-label="刷新"]');
  await until(()=>!!q('[aria-label="查看详情 Calls detail fixture"]'),"actual call in table");
  await click('[aria-label="查看详情 Calls detail fixture"]');
  await until(()=>!!q('.call-info-grid'),"details metadata");
  check(q('.call-info-grid').textContent?.includes('native-mock'),"actual_model_in_details");
  await click('#call-tab-button-request');
  await until(()=>!!q('[data-call-request]'),"request payload loaded");
  check(q('[data-call-request]').textContent?.includes('native-mock'),"actual_request_body_tab");
  check(!q('[data-call-headers]').textContent?.includes('SYNTHETIC_CALLS_KEY'),"request_headers_mask_key");
  await click('#call-tab-button-response');
  check(q('[data-call-response]')?.textContent?.includes('data:'),"actual_response_stream_tab");
  await click('[aria-label="关闭弹窗"]');
  const main=q('main'), original=q('.calls-panel'); original.style.display='none';
  const host=document.createElement('div');host.style.cssText='display:flex;flex:1;min-height:0;min-width:0';main.appendChild(host);scope=host;const root=createRoot(host);
  const records:CallRecord[]=Array.from({length:205},(_,n)=>({...result,id:`fixture-${n}`,name:`调用 ${n} ${'长名称'.repeat(30)}`,startedAt:Date.now()-n*1000,info:{...result.info,model:`model-${n}`}}));
  const render=(rows:CallRecord[])=>root.render(<Calls records={rows} onDetail={()=>{}} onClear={()=>{}} disabled={false}/>);
  render(records);await wait();
  check(host.querySelectorAll('.call-row').length===20,"pagination_default_20");
  await click('[aria-label="最后一页"]');check(host.querySelectorAll('.call-row').length===5 && q('[data-calls-page]').textContent?.includes('11 / 11'),"last_page_partial");
  await click('[aria-label="上一页"]');check(q('[data-calls-page]').textContent?.includes('10 / 11'),"previous_page");
  await click('[aria-label="第一页"]');await click('[aria-label="下一页"]');check(q('[data-calls-page]').textContent?.includes('2 / 11'),"first_next_page");
  await select('每页条数','50 条/页');check(host.querySelectorAll('.call-row').length===50 && q('[data-calls-page]').textContent?.includes('1 / 5'),"page_size_50_resets");
  await select('每页条数','100 条/页');check(host.querySelectorAll('.call-row').length===100,"page_size_100");
  await click('[aria-label="最后一页"]');render(records.slice(0,3));await wait();check(q('[data-calls-page]').textContent?.includes('1 / 1'),"shrinking_records_clamps_page");
  render(records);await wait();await select('每页条数','20 条/页');
  for(const theme of ['light','dark'] as const) {
    await api.settings({...state.settings,theme});await click('[aria-label="刷新"]');
    for(const [width,height] of [[960,640],[820,550],[800,520]]) {
      await invoke('smoke_resize',{width,height});await until(()=>innerWidth===width && innerHeight===height,'resize');await wait();
      const tag=`${width}x${height}_${theme}`, row=host.querySelector<HTMLElement>('.call-row')!, head=host.querySelector<HTMLElement>('thead tr')!, footer=host.querySelector<HTMLElement>('.calls-pagination')!;
      check(Math.abs(row.getBoundingClientRect().height-36)<=1,'row_36px_'+tag);
      check(Math.abs(head.getBoundingClientRect().height-33)<=1,'header_33px_'+tag);
      check(main.scrollWidth<=main.clientWidth+1 && main.scrollHeight<=main.clientHeight+1,'page_no_outer_overflow_'+tag);
      check(footer.getBoundingClientRect().bottom<=q('.statusbar').getBoundingClientRect().top,'pagination_visible_'+tag);
      const scroll=host.querySelector<HTMLElement>('.calls-scroll')!, action=host.querySelector<HTMLElement>('tbody .call-action')!;
      const right=action.getBoundingClientRect().right;scroll.scrollLeft=600;await wait();
      check(Math.abs(right-action.getBoundingClientRect().right)<=1,'action_sticky_right_'+tag);
      check(action.getBoundingClientRect().right<=scroll.getBoundingClientRect().right+1,'action_inside_table_'+tag);scroll.scrollLeft=0;
    }
  }
  root.unmount();host.remove();scope=null;original.style.display='';
  await api.settings({...state.settings,detailedLogging:false,theme:'light'});await invoke('smoke_resize',{width:960,height:640});
  await api.remove(id);await click('[aria-label="刷新"]');
  return checks;
}
