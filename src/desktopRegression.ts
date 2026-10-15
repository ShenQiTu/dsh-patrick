import { api } from "./api";
import { invoke } from "@tauri-apps/api/core";
import { emptyModel } from "./types";
export async function runDesktopRegression() {
  const checks: string[] = [];
  const sleep = () => new Promise(r=>setTimeout(r,80));
  const q=(s:string)=>document.querySelector<HTMLElement>(s)!;
  function check(ok: unknown, name: string) { if(!ok) throw new Error("DESKTOP: "+name); checks.push(name); }
  async function click(s:string) { if(!q(s)) throw new Error("DESKTOP missing: "+s); q(s).click(); await sleep(); }
  async function until(fn:()=>boolean|Promise<boolean>) { for(let i=0;i<100;i++){ if(await fn())return; await sleep(); } throw new Error("DESKTOP timeout"); }
  async function input(selector:string, value:string) {
    const el=q(selector) as HTMLInputElement;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(el,value);
    el.dispatchEvent(new Event("input",{bubbles:true})); await sleep();
  }
  async function preset(label:string) {
    Array.from(document.querySelectorAll<HTMLElement>('.overview-range button')).find(e=>e.textContent===label)!.click(); await sleep();
  }
  function fit(label:string) {
    const main=q('main'), r=main.getBoundingClientRect();
    check(main.scrollHeight<=main.clientHeight+1 && main.scrollWidth<=main.clientWidth+1,
      `overview_no_scroll_${label}_height_${main.scrollHeight}/${main.clientHeight}`);
    const blocks=['.usage-toolbar','.token-chart','.token-bars','.token-axis','.usage-metrics','.usage-calendar','.calendar-axis'].map(q);
    check(blocks.every(el=>{const b=el.getBoundingClientRect();return b.height>0 && b.top>=r.top-1 && b.bottom<=r.bottom+1 && b.left>=r.left-1 && b.right<=r.right+1;}),`overview_all_blocks_visible_${label}`);
    check(['.usage-toolbar','.token-chart','.usage-metrics','.usage-calendar'].map(q).every((el,i,a)=>i===0 || a[i-1].getBoundingClientRect().bottom<=el.getBoundingClientRect().top),`overview_blocks_no_overlap_${label}`);
    check(Array.from(document.querySelectorAll<HTMLElement>('.usage-metrics section')).every(section=>{
      const b=section.getBoundingClientRect(), children=Array.from(section.children) as HTMLElement[];
      return children.every((el,i)=>{
        const c=el.getBoundingClientRect();
        return c.top>=b.top && c.bottom<=b.bottom && c.left>=b.left && c.right<=b.right && el.scrollWidth<=el.clientWidth+1
          && (i===0 || children[i-1].getBoundingClientRect().bottom<=c.top+1);
      });
    }),`overview_metric_contents_complete_${label}`);
    check(Array.from(document.querySelectorAll<HTMLElement>('.usage-metrics small,.token-axis,.calendar-axis,.token-legend')).every(el=>parseFloat(getComputedStyle(el).fontSize)>=12),`overview_auxiliary_font_min_12_${label}`);
  }
  for (const page of ["models","overview","calls","connection","settings","help"]) {
    await click(`[data-nav="${page}"]`);
    check(!!q('[role="switch"][aria-label="启动或关闭网关"]') === (page==="models"), `gateway_control_only_models_${page}`);
  }
  await click('[data-nav="connection"]');
  check(!!q('input[aria-label="DSH profile 目录"]'),"dsh_profile_setting_visible");
  check(!q('[data-scan-dsh]')&&!q('[aria-label="DSH 扫描起点"]'),"manual_profile_no_scan_controls");
  check((await api.snapshot()).settings.dshProfile==="","new_install_profile_blank_no_guessed_directory");
  check(await invoke("smoke_lifecycle",{action:"bridge-self-contained"}),"packaged_bridge_own_runtime_parser_sync_exact_restore");
  check(q('.connection-panel').textContent?.includes("保存路径不代表检查通过"),"dsh_configured_not_claimed_as_runtime_loaded");
  check(Array.from(document.querySelectorAll<HTMLButtonElement>('.connection-buttons button')).filter(b=>["检查状态","重新同步","断开并恢复"].includes(b.textContent||"")).every(b=>b.disabled),"smoke_cannot_touch_formal_dsh");
  check(q('[data-dsh-stage="directory"]').dataset.state==="unchecked"&&q('[data-dsh-stage="sync"]').dataset.state==="unchecked","diagnostics_initial_not_falsely_green");
  const initialSettings=(await api.snapshot()).settings;
  for(const scenario of ["invalid","valid","synced"]) {
    await invoke("smoke_lifecycle",{action:`diagnostics-${scenario}`});await click('[aria-label="刷新"]');
    await until(()=>q('[data-dsh-stage="directory"]').dataset.state===(scenario==="invalid"?"fail":"pass"));
    check(q('[data-dsh-stage="sync"]').dataset.state===(scenario==="synced"?"pass":"unchecked"),`diagnostics_sync_separate_${scenario}`);
    if(scenario==="invalid") {
      check(q('[data-dsh-item="yaml"]')?.dataset.state==="fail"&&q('[data-dsh-item="providers"]')?.dataset.state==="unchecked","diagnostics_failed_stage_and_unchecked_dependents");
      check((q('.connection-check-details') as HTMLDetailsElement).open,"diagnostics_failure_details_auto_expand");
      check(q('.connection-diagnostics').textContent?.includes('cordis.patch.yml')&&!q('.connection-panel').textContent?.includes('SYNTHETIC_SECRET'),"diagnostics_ui_file_location_no_secret");
    }
    check(q('[data-dsh-stage="client"]').textContent?.includes('网关未运行'),`diagnostics_no_client_claim_${scenario}`);
    for(const [width,height] of [[960,640],[800,520]]) {
      await invoke("smoke_resize",{width,height});await until(()=>innerWidth===width&&innerHeight===height);
      for(const theme of ["light","dark"] as const) {
        await api.settings({...(await api.snapshot()).settings,theme});await click('[aria-label="刷新"]');await until(()=>document.documentElement.dataset.theme===theme);
        const panel=q('.connection-panel').getBoundingClientRect();
        check(Array.from(document.querySelectorAll<HTMLElement>('[data-dsh-stage],.connection-check-details')).every(el=>{const b=el.getBoundingClientRect();return b.left>=panel.left&&b.right<=panel.right+1&&el.scrollWidth<=el.clientWidth+1;}),`diagnostics_contained_${scenario}_${width}_${height}_${theme}`);
      }
    }
    if(scenario==="valid") {
      await input('input[aria-label="DSH profile 目录"]','F:\\unsaved\\profiles\\desktop');
      check(q('[data-dsh-stage="directory"]').dataset.state==="unchecked"&&q('[data-dsh-path-hint]').textContent?.includes('尚未保存'),"diagnostics_unsaved_path_no_stale_result");
    }
  }
  await api.settings(initialSettings);await click('[aria-label="刷新"]');
  check((await api.snapshot()).dsh.checkedAt===0&&q('[data-dsh-stage="directory"]').dataset.state==="unchecked","diagnostics_saved_path_reset");
  await invoke("smoke_resize",{width:960,height:640});await until(()=>innerWidth===960&&innerHeight===640);
  await click('[data-nav="overview"]');
  await until(()=>!!q('[data-usage-calls]'));
  const stats=await api.statistics();
  const gateways=stats.filter(r=>r.source==="gateway");
  await until(()=>q('[data-usage-calls]').textContent === String(gateways.length));
  check(true,"overview_defaults_to_real_gateway_records");
  await invoke("smoke_resize", {width:820,height:550});
  await until(()=>innerWidth===820 && innerHeight===550);
  await sleep();
  check(q('main').scrollHeight <= q('main').clientHeight + 1,
    `overview_one_screen_820x550_scroll_${q('main').scrollHeight}_client_${q('main').clientHeight}`);
  check(q('[data-overview]').textContent?.includes("未配置计价"),"overview_no_invented_price");
  check(q('.usage-calendar').querySelectorAll('.calendar-squares span').length===365,"overview_365_day_calendar");
  await click('[aria-label="统计请求来源"]');
  const option=Array.from(document.querySelectorAll<HTMLElement>('[role="option"]')).find(e=>e.textContent==="模型测试")!;option.click();await sleep();
  check(q('[data-usage-calls]').textContent===String(stats.filter(r=>r.source==="test").length),"overview_separates_model_tests");
  const custom=Array.from(document.querySelectorAll<HTMLElement>('.overview-range button')).find(e=>e.textContent==="自定义")!;custom.click();await sleep();
  check(!!q('[aria-label="统计开始时间"]') && !!q('[aria-label="统计结束时间"]'),"overview_custom_range_controls");
  await input('[aria-label="统计开始时间"]','2090-01-02T00:00');
  await input('[aria-label="统计结束时间"]','2090-01-01T00:00');
  (q('form.custom-range') as HTMLFormElement).requestSubmit(); await sleep();
  check(q('form.custom-range [role="alert"]')?.textContent==="开始时间须早于结束时间","overview_custom_invalid_range_rejected");
  await click('[aria-label="关闭弹窗"]');
  for (const [width,height] of [[960,640],[820,550],[800,520]]) {
    await invoke("smoke_resize",{width,height});await until(()=>innerWidth===width && innerHeight===height);
    for (const theme of ["light","dark"] as const) {
      await api.settings({...(await api.snapshot()).settings,theme});await click('[aria-label="刷新"]');
      await until(()=>document.documentElement.dataset.theme===theme);
      await preset('近一个月');
      check(Number(q('[data-usage-calls]').textContent)>0,`overview_populated_fixture_${width}_${height}_${theme}`);
      fit(`${width}x${height}_${theme}_populated`);
      await preset('自定义');
      await input('[aria-label="统计开始时间"]','2090-01-01T00:00');
      await input('[aria-label="统计结束时间"]','2090-01-02T00:00');
      (q('form.custom-range') as HTMLFormElement).requestSubmit(); await until(()=>!q('dialog[open]'));
      check(q('[data-usage-calls]').textContent==='0' && !!q('.chart-empty'),`overview_empty_fixture_${width}_${height}_${theme}`);
      fit(`${width}x${height}_${theme}_empty`);
    }
  }
  for (const label of ['近1小时','近1自然日','近10分钟','近一周','近一个月']) { await preset(label);fit(`preset_${label}`); }
  await invoke("smoke_resize",{width:960,height:640});await until(()=>innerWidth===960 && innerHeight===640);
  await api.settings({...(await api.snapshot()).settings,theme:'light'});await click('[aria-label="刷新"]');
  await click('[data-nav="models"]');
  const longPrompt="中".repeat(17*1024);
  const id=await api.save({...emptyModel(),name:"Prompt switch desktop",model:"native-mock",baseUrl:(await api.snapshot()).testUpstream!,prompt:longPrompt,promptEnabled:true});
  await click('[aria-label="刷新"]');
  await until(()=>!!q('[aria-label="编辑 Prompt switch desktop"]'));
  await click('[aria-label="编辑 Prompt switch desktop"]');
  const textarea=q('textarea[aria-label="注入提示词"]') as HTMLTextAreaElement;
  check(textarea.maxLength===-1,"prompt_no_mismatched_character_limit");
  check(q('[data-prompt-size]').textContent?.includes("52224 / 131072"),"prompt_51k_utf8_counter");
  const setPrompt=async(value:string)=>{
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,"value")!.set!.call(textarea,value);
    textarea.dispatchEvent(new Event("input",{bubbles:true}));await sleep();
  };
  await setPrompt("a".repeat(131073));
  (q('form.compact-editor') as HTMLFormElement).requestSubmit();await sleep();
  check(q('[role="alert"]')?.textContent?.includes("注入提示词过长：当前 131073 字节，上限 131072 字节"),"prompt_overlimit_error_identifies_field_and_bytes");
  check(document.activeElement===textarea,"prompt_error_focuses_field");
  check((await api.snapshot()).models.find(m=>m.id===id)?.prompt===longPrompt,"prompt_rejected_draft_preserves_saved_text");
  await setPrompt(longPrompt);
  await click('[aria-label="启用注入提示词"]');
  check(!q('textarea[aria-label="注入提示词"]'),"prompt_disabled_editor_collapses");
  (q('form.compact-editor') as HTMLFormElement).requestSubmit();
  await until(()=>!q('dialog[open]'));
  let m=(await api.snapshot()).models.find(m=>m.id===id)!;
  check(!m.promptEnabled && m.prompt===longPrompt,"prompt_disable_saves_flag_preserves_text");
  const duplicated=await api.duplicate(id);
  m=(await api.snapshot()).models.find(m=>m.id===duplicated)!;
  check(!m.promptEnabled && m.prompt===longPrompt,"prompt_duplicate_preserves_switch");
  await click('[aria-label="刷新"]');
  await click('[aria-label="编辑 Prompt switch desktop"]');
  await click('[aria-label="启用注入提示词"]');
  check((q('textarea[aria-label="注入提示词"]') as HTMLTextAreaElement).value===longPrompt,"prompt_reenable_restores_text");
  (q('form.compact-editor') as HTMLFormElement).requestSubmit();await until(()=>!q('dialog[open]'));
  check((await api.snapshot()).models.find(m=>m.id===id)?.promptEnabled,"prompt_reenable_persisted");
  await api.remove(duplicated); await api.remove(id); await click('[aria-label="刷新"]');
  check(await invoke("smoke_lifecycle", {action:"exit-check"}),"tray_exit_allowed_after_requests_finish");
  const pause=(await api.snapshot()).testPauseSeconds;
  if (pause) {
    await click('[data-nav="overview"]');
    await invoke("smoke_resize",{width:820,height:550}); await until(()=>innerWidth===820 && innerHeight===550);
    await new Promise(r=>setTimeout(r,pause*1000));
  }
  return checks;
}
