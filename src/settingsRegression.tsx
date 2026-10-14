import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { api } from "./api";
import { DshConnection } from "./DshConnection";
import { emptyModel } from "./types";

export async function runSettingsRegression() {
  const checks: string[] = [];
  const sleep = () => new Promise(r => setTimeout(r, 80));
  const q = (s: string) => document.querySelector<HTMLElement>(s)!;
  function check(ok: unknown, name: string) { if (!ok) throw new Error("SETTINGS: " + name); checks.push(name); }
  async function until(fn: () => boolean | Promise<boolean>, name: string) { for (let i = 0; i < 100; i++) { if (await fn()) return; await sleep(); } throw new Error("SETTINGS timeout: " + name); }
  async function click(s: string) { check(!!q(s), "control_exists_" + s); q(s).click(); await sleep(); }
  async function input(s: string, value: string) {
    const el = q(s) as HTMLInputElement;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true })); await sleep();
  }
  await click('[data-nav="settings"]');
  const originalSettings=(await api.snapshot()).settings;
  const timeout=q('[name="timeoutSeconds"]') as HTMLInputElement;
  check(timeout.min==='5' && timeout.max==='700', 'timeout_range_5_to_700');
  for(const value of ['4','701','699.5']) {
    await input('[name="timeoutSeconds"]',value);
    check(!timeout.checkValidity(),'timeout_rejects_'+value);
  }
  await input('[name="timeoutSeconds"]','700');
  check(timeout.checkValidity(),'timeout_accepts_700');
  (q('form.settings-groups') as HTMLFormElement).requestSubmit();
  await until(async()=> (await api.snapshot()).settings.timeoutSeconds===700,'timeout persisted through settings form');
  await click('[aria-label="刷新"]');
  check((q('[name="timeoutSeconds"]') as HTMLInputElement).value==='700','timeout_700_saved_and_reloaded');
  let rejected=false;
  try {await api.settings({...originalSettings,timeoutSeconds:701});} catch {rejected=true;}
  check(rejected,'timeout_backend_rejects_701');
  check((await api.snapshot()).settings.timeoutSeconds===700,'timeout_invalid_save_preserves_700');
  await api.settings(originalSettings);await click('[aria-label="刷新"]');
  check(!!q('[data-settings-group="stream-timeout"]'), 'stream_timeout_separate_group');
  check(q('[data-stream-wait]').textContent?.includes('610 秒'), 'stream_wait_default_600_plus_10');
  for (const [field,bad] of [['streamIdleSeconds','4'],['streamIdleSeconds','7201'],['streamGraceSeconds','0'],['streamGraceSeconds','61']]) {
    await input(`[name="${field}"]`,bad);
    check(!(q(`[name="${field}"]`) as HTMLInputElement).checkValidity(),'stream_invalid_'+field+'_'+bad);
  }
  await input('[name="streamIdleSeconds"]','180');await input('[name="streamGraceSeconds"]','10');
  check(q('[data-stream-wait]').textContent?.includes('190 秒'),'stream_effective_wait_preview');
  (q('form.settings-groups') as HTMLFormElement).requestSubmit();
  await until(async()=> (await api.snapshot()).settings.streamIdleSeconds===180, 'stream wait saved');
  await click('[aria-label="刷新"]');
  check((q('[name="streamGraceSeconds"]') as HTMLInputElement).value==='10', 'stream_grace_saved_and_reloaded');
  let invalidStream=false;
  try {await api.settings({...originalSettings,streamIdleSeconds:7201});} catch {invalidStream=true;}
  check(invalidStream && (await api.snapshot()).settings.streamIdleSeconds===180, 'stream_backend_validation_preserves_previous');
  await api.settings(originalSettings);await click('[aria-label="刷新"]');
  for (const s of [".pricing-settings", ".application-settings", ".storage-settings"]) check(!!q(s), s + "_rendered");
  check(!q("form form"), "independent_forms_not_nested");
  check(getComputedStyle(q('[data-price="input"]')).padding === '0px', "price_value_no_global_muted_padding");
  const visualPause = (await api.snapshot()).testPauseSeconds ?? 0;
  if (visualPause) {
    q('.pricing-settings').scrollIntoView({block:'start'});
    await new Promise(r=>setTimeout(r,visualPause*1000));
  }
  await click('[data-edit-pricing]');
  await input('[name="price-input"]', '123');
  await click('[aria-label="刷新"]');
  check((q('[name="price-input"]') as HTMLInputElement).value === '123', "price_draft_survives_snapshot_refresh");
  await click('[data-cancel-pricing]');
  check((await api.snapshot()).pricing.input === null, "price_cancel_does_not_save");
  await click('[data-edit-pricing]');
  for (const [name, value] of [["input", "5"], ["output", "25"], ["cacheRead", "0.5"], ["cacheWrite", "6.25"]]) await input(`[name="price-${name}"]`, value);
  await click('[data-save-pricing]');
  await until(() => !!q('[data-price="input"]'), "pricing saved");
  check(q('[data-price="input"]').textContent === '$5' && (await api.snapshot()).pricing.cacheWrite === 6.25, "pricing_ui_saved_and_reloaded");
  // Gateway settings are a separate write; they must not discard the pricing card's values.
  await api.settings((await api.snapshot()).settings);
  check((await api.snapshot()).pricing.output === 25, "gateway_settings_preserve_prices");
  await click('[data-nav="overview"]');
  await until(() => !!q('[data-usage-cost]') && q('[data-usage-cost]').textContent!.startsWith('$'), "overview estimated cost");
  check(q('[data-usage-cost]').nextElementSibling?.textContent?.includes('估算覆盖'), "overview_shows_estimate_coverage");
  await api.savePricing({input:null,output:null,cacheRead:null,cacheWrite:null});
  await click('[aria-label="刷新"]');
  await until(() => q('[data-usage-cost]')?.textContent === '—', "unconfigured price restored");
  await click('[data-nav="settings"]');
  await until(() => !(q('[aria-label="开机启动"]') as HTMLButtonElement)?.disabled, "read autostart");
  await click('[aria-label="开机启动"]');
  await until(async () => (await api.autostartStatus()).enabled, "startup enabled");
  check((await api.autostartStatus()).currentExecutable, "autostart_isolated_registry_current_exe");
  await click('[aria-label="开机启动"]');
  await until(async () => !(await api.autostartStatus()).enabled, "startup disabled");
  check(q('[aria-label="开机启动"]').getAttribute('aria-checked') === 'false', "autostart_switch_restored_off");
  await click('[data-check-update]');
  await until(() => !!q('[data-update-result]'), "update result");
  check(q('[data-update-result]').textContent?.includes('自测模式不访问 GitHub') && !(q('[data-check-update]') as HTMLButtonElement).disabled, "update_error_visible_loading_ends_no_network");
  const id = await api.save({...emptyModel(),name:'Storage preserved model',model:'native-mock',baseUrl:(await api.snapshot()).testUpstream!,apiKey:'SYNTHETIC_STORAGE_KEY'});
  await click('[aria-label="刷新"]');
  const configBefore = (await api.snapshot()).models;
  const storageBefore = await api.statisticsStorage();
  check(storageBefore.calls > 0, "storage_has_fixture_records");
  await click('[data-clear-storage]'); await click('[data-cancel-clear]');
  check((await api.statisticsStorage()).calls === storageBefore.calls, "storage_cancel_preserves_records");
  await click('[data-clear-storage]'); await click('[data-confirm-clear]');
  await until(() => !q('dialog[open]'), "storage cleared");
  check((await api.statisticsStorage()).calls === 0 && (await api.statistics()).length === 0, "storage_confirm_clears_statistics");
  const after = await api.snapshot();
  check(after.runtime.calls.length === 0 && Object.keys(after.runtime.testResults).length === 0, "storage_clears_session_results");
  check(JSON.stringify(after.models) === JSON.stringify(configBefore) && after.models.some(m => m.id === id && m.hasKey), "storage_preserves_models_and_credentials");
  check(q('[data-storage-count]').textContent?.includes('统计记录 0 条'), "storage_count_refreshes");
  await api.remove(id);
  // Render real component with synthetic long paths, without writing any DSH profile.
  const sample = await api.snapshot();
  sample.settings.dshProfile = 'F:\\synthetic-long-profile\\' + '目录路径'.repeat(30);
  sample.dsh.backupDirectory = sample.settings.dshProfile + '\\.dsh-patrick';
  await click('[data-nav="connection"]');
  const main = q('main');
  const host = document.createElement('div'); main.appendChild(host);
  const original = main.querySelector<HTMLElement>(':scope > .connection-panel')!;
  original.style.display = 'none';
  const root = createRoot(host);
  root.render(<DshConnection snapshot={sample} busy={false} action={async () => { throw new Error('fixture actions must not execute'); }} />);
  await sleep();
  for (const theme of ['light','dark'] as const) {
    await api.settings({...sample.settings,dshProfile:(await api.snapshot()).settings.dshProfile,theme});
    await click('[aria-label="刷新"]');
    for (const [width,height] of [[960,640],[820,550],[800,520]]) {
      await invoke('smoke_resize',{width,height}); await until(() => innerWidth===width && innerHeight===height, 'resize'); await sleep();
      const panel=host.querySelector<HTMLElement>('.connection-panel')!, p=panel.getBoundingClientRect();
      const content=host.querySelector<HTMLElement>('.connection-content')!;
      const profile=host.querySelector<HTMLElement>('.connection-profile')!.getBoundingClientRect();
      const backup=host.querySelector<HTMLElement>('.connection-recovery .address-row')!.getBoundingClientRect();
      const tag=`${width}x${height}_${theme}`;
      check(profile.width<=520.5 && profile.left-p.left>=16 && p.right-profile.right>=16, 'profile_bounded_padded_'+tag);
      check(backup.left-p.left>=16 && p.right-backup.right>=16 && p.bottom-backup.bottom>=16, 'backup_inside_card_with_bottom_padding_'+tag);
      check(content.scrollWidth<=content.clientWidth+1 && main.scrollWidth<=main.clientWidth+1, 'connection_no_horizontal_overflow_'+tag);
      const children=Array.from(content.children) as HTMLElement[];
      check(children.every((el,i)=>i===0 || children[i-1].getBoundingClientRect().bottom<=el.getBoundingClientRect().top+1), 'connection_sections_no_overlap_'+tag);
      check(children.every(el=>el.getBoundingClientRect().bottom<=p.bottom-15), 'connection_background_contains_content_'+tag);
      check(getComputedStyle(host.querySelector<HTMLElement>('.connection-recovery > p')!).padding==='0px', 'recovery_text_aligned_no_double_padding_'+tag);
      if (visualPause && width===820 && theme==='light') {
        main.scrollTop=0;
        await new Promise(r=>setTimeout(r,visualPause*1000));
      }
    }
  }
  root.unmount();host.remove();original.style.display='';
  await api.settings({...sample.settings,dshProfile:(await api.snapshot()).settings.dshProfile,theme:'light'});
  await invoke('smoke_resize',{width:960,height:640});
  await click('[aria-label="刷新"]');
  return checks;
}
