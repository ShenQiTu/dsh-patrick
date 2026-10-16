import { invoke } from "@tauri-apps/api/core";
import { api } from "./api";
import { emptyModel, type Protocol } from "./types";

export async function runUIRegression(): Promise<string[]> {
  const checks: string[] = [];
  const q = (s: string) => document.querySelector<HTMLElement>(s)!;
  const all = (s: string) =>
    Array.from(document.querySelectorAll<HTMLElement>(s));
  const sleep = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const assert = (value: unknown, name: string) => {
    if (!value) throw new Error("UI_REGRESSION: " + name);
    checks.push(name);
  };
  async function until(f: () => unknown | Promise<unknown>, name: string) {
    for (let n = 0; n < 120; n++) {
      if (await f()) return;
      await sleep(40);
    }
    throw new Error("UI_REGRESSION timeout: " + name);
  }
  async function click(s: string) {
    await until(() => q(s), s);
    q(s).click();
    await sleep();
  }
  async function input(s: string, value: string) {
    const e = q(s);
    Object.getOwnPropertyDescriptor(
      e instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype,
      "value",
    )!.set!.call(e, value);
    e.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep();
  }
  async function refresh() {
    await click('[aria-label="刷新"]');
    await sleep();
  }
  async function page(p: string) {
    await click(`[data-nav="${p}"]`);
    q("main").scrollTop = 0;
  }
  async function size(width: number, height: number) {
    await invoke("smoke_resize", { width, height });
    await until(
      () => innerWidth === width && innerHeight === height,
      "native size",
    );
  }
  async function theme(value: "light" | "dark") {
    await api.settings({ ...(await api.snapshot()).settings, theme: value });
    await refresh();
    await until(
      () => document.documentElement.dataset.theme === value,
      "theme",
    );
  }
  async function openEditor() {
    await page("models");
    await click("[data-add-model]");
  }
  async function close() {
    await click('[aria-label="关闭弹窗"]');
  }
  async function choose(label: string, text: string) {
    await click(`[role="combobox"][aria-label="${label}"]`);
    const option = all('[role="option"]').find(
      (e) => e.textContent?.trim() === text,
    );
    if (!option) throw new Error("Missing option " + text);
    option.click();
    await sleep();
  }
  const state = await api.snapshot(),
    upstream = state.testUpstream!;
  await size(960, 640);
  await theme("light");
  await openEditor();
  assert((q('[name="compactionPercent"]') as HTMLInputElement).value==='85','compaction_default_85');
  assert(q('[aria-label="多模态（识图）"]').getAttribute('aria-checked')==='false','new_model_vision_default_off');
  await input('[name="compactionPercent"]','90');
  await click('[aria-label="多模态（识图）"]');
  assert(q('[aria-label="多模态（识图）"]').getAttribute('aria-checked')==='true','vision_switch_works');
  assert(!q('dialog')!.textContent?.includes('有效触发点'),'no_compaction_budget_preview');
  await input('[name="baseUrl"]', upstream);
  await click("[data-discover-models]");
  await until(() => all('[role="option"]').length === 2, "discovery");
  await input('[name="model"]', "zzzz_no_match");
  assert(
    all('[role="option"]').length === 0 && !!q(".choice-empty"),
    "F02_no_match_empty_state",
  );
  q('[name="name"]').dispatchEvent(
    new PointerEvent("pointerdown", { bubbles: true }),
  );
  q('[name="name"]').focus();
  await sleep();
  assert(!q(".choice-menu"), "F02_outside_pointer_closes");
  q('[name="model"]').focus();
  await input('[name="model"]', "native");
  await input('[name="name"]', "Keyboard regression");
  const modelInput = q('[name="model"]');
  modelInput.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "ArrowDown",
      bubbles: true,
      cancelable: true,
    }),
  );
  await sleep();
  assert(
    !!modelInput.getAttribute("aria-activedescendant"),
    "F01_arrow_active_descendant",
  );
  const enter = new KeyboardEvent("keydown", {
    key: "Enter",
    bubbles: true,
    cancelable: true,
  });
  modelInput.dispatchEvent(enter);
  await sleep();
  assert(
    enter.defaultPrevented && !!q("dialog[open]"),
    "F01_enter_selects_without_form_submission",
  );
  assert(!q(".choice-menu"), "F01_selection_closes_menu");
  assert(
    (modelInput as HTMLInputElement).value.startsWith("native-mock") &&
      (await api.snapshot()).models.length === 0,
    "F01_selected_model_not_partial_saved",
  );
  await click('[aria-label="展开模型列表"]');
  q('[name="name"]').focus();
  // Hidden WebView may update activeElement without dispatching a native focus event.
  q('[name="name"]').dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
  await sleep();
  assert(!q(".choice-menu"), "F02_keyboard_focus_closes");
  await click('[aria-label="展开模型列表"]');
  modelInput.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    }),
  );
  await sleep();
  assert(
    !q(".choice-menu") && !!q("dialog[open]"),
    "F02_escape_closes_only_menu",
  );
  await close();

  await openEditor();
  await input('[name="baseUrl"]', "http://127.0.0.1:1/v1");
  await click("[data-discover-models]");
  await until(() => q('[role="alert"]'), "visible discovery error");
  const error = q('[role="alert"]').getBoundingClientRect(),
    scroll = q(".editor-scroll").getBoundingClientRect();
  assert(
    error.top >= 0 && error.bottom <= scroll.top + 1,
    "F03_error_visible_above_scroll",
  );
  await close();
  for (const kind of ["Headers", "Extra"]) {
    await openEditor();
    await input('[name="baseUrl"]', upstream);
    await input('[name="model"]', "native-mock");
    await input('[name="name"]', kind + " disabled JSON");
    const label = kind === "Headers" ? "自定义 Headers" : "额外参数";
    await click(`[role="switch"][aria-label="${label}"]`);
    await input(`[aria-label="${label} JSON"]`, "{");
    (q("form.compact-editor") as HTMLFormElement).requestSubmit();
    await sleep();
    assert(
      !!q('[role="alert"]')?.textContent?.includes("格式错误") &&
        document.activeElement === q(`[aria-label="${label} JSON"]`),
      `F04_${kind}_enabled_json_error_located`,
    );
    await click(`[role="switch"][aria-label="${label}"]`);
    (q("form.compact-editor") as HTMLFormElement).requestSubmit();
    await until(() => !q("dialog[open]"), "disabled invalid draft saved");
    const m = (await api.snapshot()).models.find(
      (m) => m.name === kind + " disabled JSON",
    )!;
    assert(
      !!m && !(kind === "Headers" ? m.headersEnabled : m.extraEnabled),
      `F04_${kind}_disabled_invalid_draft_does_not_block`,
    );
    await api.remove(m.id);
    await refresh();
  }
  const kept = await api.save({
    ...emptyModel(),
    name: "Keep disabled values",
    model: "native-mock",
    baseUrl: upstream,
    headers: { "x-test": "preserved" },
    extraParams: { temperature: 0.25 },
  });
  await refresh();
  await click('[aria-label="编辑 Keep disabled values"]');
  (q("form.compact-editor") as HTMLFormElement).requestSubmit();
  await until(() => !q("dialog[open]"), "preserve disabled values");
  let saved = (await api.snapshot()).models.find((m) => m.id === kept)!;
  assert(
    saved.headers["x-test"] === "preserved" &&
      saved.extraParams.temperature === 0.25,
    "F04_disabled_valid_values_preserved",
  );
  await api.remove(kept);
  await refresh();

  for (const content of ["empty", "populated"]) {
    if (content === "populated") {
      const id = await api.save({
        ...emptyModel(),
        name: "长名称审核模型_" + "Patrick_Model_".repeat(7),
        model: "native-mock",
        baseUrl: upstream,
        apiKey: "NATIVE_MOCK_KEY",
      });
      await api.test(id);
    }
    for (const t of ["light", "dark"] as const)
      for (const [width, height] of [
        [960, 640],
        [800, 520],
      ]) {
        await size(width, height);
        await theme(t);
        for (const p of [
          "models",
          "overview",
          "calls",
          "connection",
          "settings",
          "help",
        ]) {
          await page(p);
          assert(
            q("main").scrollWidth <= q("main").clientWidth + 1 &&
              document.documentElement.scrollWidth <= innerWidth,
            `layout_${content}_${t}_${width}_${p}_no_horizontal_overflow`,
          );
        }
      }
  }
  await page("overview");
  assert(!!q("[data-overview]") && !q(".welcome-card"), "dashboard_replaces_welcome_card");
  await page("models");
  const success = q(".test-status.success"),
    fg = getComputedStyle(success).color.match(/\d+/g)!.map(Number),
    bg = getComputedStyle(success).backgroundColor.match(/\d+/g)!.map(Number);
  const lum = (c: number[]) =>
    c
      .map((n) => n / 255)
      .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
      .reduce((v, n, i) => v + n * [0.2126, 0.7152, 0.0722][i], 0);
  const a = lum(fg),
    b = lum(bg);
  assert(
    (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05) >= 4.5,
    "F06_dark_success_contrast_at_least_4_5",
  );
  await openEditor();
  assert(
    getComputedStyle(q(".preset-monochrome")).filter ===
      "brightness(0) invert(1)",
    "F06_dark_kimi_adapted",
  );
  const beforeEye = q('[aria-label="显示新 Key"]').innerHTML;
  await click('[aria-label="显示新 Key"]');
  assert(
    beforeEye !== q('[aria-label="隐藏新 Key"]').innerHTML &&
      q('[name="apiKey"]').getAttribute("type") === "text",
    "F08_eye_icon_toggles",
  );
  all(".preset-row button")
    .find((e) => e.textContent === "DeepSeek")!
    .click();
  await sleep();
  assert(
    q('.preset-row button[aria-pressed="true"]')?.textContent === "DeepSeek",
    "F08_preset_active_state",
  );
  assert(
    (q('[name="model"]') as HTMLInputElement).value === "" &&
      (q('[name="contextWindow"]') as HTMLInputElement).value === "",
    "preset_does_not_prefill_model_or_tokens",
  );
  await choose("模型类型", "Anthropic");
  assert(!!q('[name="thinkingBudget"]'), "F08_type_select_changes_protocol");
  await choose("模型类型", "OpenAI");
  await choose("请求协议", "Responses API");
  assert(
    q('[aria-label="请求协议"]')?.textContent?.includes("Responses API"),
    "F08_protocol_select_changes",
  );
  const help = q(".field-help");
  help.focus();
  help.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
  await sleep();
  assert(!!q('[role="tooltip"]'), "F08_svg_help_keyboard_tooltip");
  help.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    }),
  );
  await sleep();
  assert(
    !q('[role="tooltip"]') && !!q("dialog[open]"),
    "F08_tooltip_escape_preserves_modal",
  );
  await close();

  for (const protocol of ["openai", "responses", "anthropic"] as Protocol[]) {
    await openEditor();
    if (protocol === "anthropic") await choose("模型类型", "Anthropic");
    else if (protocol === "responses")
      await choose("请求协议", "Responses API");
    for (const label of ["自定义 Headers", "额外参数", "启用注入提示词"])
      await click(`[role="switch"][aria-label="${label}"]`);
    q(".editor-scroll").scrollTop = q(".editor-scroll").scrollHeight;
    assert(
      q('[aria-label="启用模型"]').getBoundingClientRect().bottom <=
        q(".editor-scroll").getBoundingClientRect().bottom &&
        q(".modal-foot").getBoundingClientRect().bottom <= innerHeight,
      `minimum_editor_${protocol}_expanded_reachable`,
    );
    await close();
  }
  await openEditor();
  await input('[name="baseUrl"]', upstream);
  await click("[data-discover-models]");
  await until(() => q(".choice-menu"), "minimum dropdown");
  const menu = q(".choice-menu").getBoundingClientRect();
  assert(
    menu.top >= 0 && menu.bottom <= innerHeight && menu.right <= innerWidth,
    "minimum_dropdown_fits_viewport",
  );
  await close();
  await page("calls");
  await choose("筛选请求来源", "网关请求");
  assert(all(".call-row").length === 0, "custom_call_source_filter");
  await choose("筛选请求来源", "全部来源");
  assert(all(".call-row").length === 1, "custom_call_source_reset");
  await page("settings");
  await choose("界面主题", "浅色");
  await until(
    () => document.documentElement.dataset.theme === "light",
    "theme UI save",
  );
  assert(true, "custom_theme_select_persisted");

  // Optional visible review, using fake local data only. No production configuration is loaded.
  if (state.testPauseSeconds) {
    await size(960, 640);
    await openEditor();
    await input('[name="baseUrl"]', upstream);
    await click("[data-discover-models]");
    await until(() => q(".choice-menu"), "visual dropdown");
    await input('[name="model"]', "native");
    await input('[name="name"]', "Real Enter review");
    q('[name="model"]').focus();
    // Wait for an actual keyboard choice; do not mark an unattended pause as a keyboard pass.
    for (let n = 0; n < 600 && !!q('dialog[open]') && (q('[name="model"]') as HTMLInputElement).value === "native"; n++) await sleep(100);
    assert(!!q("dialog[open]") && (q('[name="model"]') as HTMLInputElement).value === "native-mock", "real_enter_selects_candidate_without_submit");
    assert(!(await api.snapshot()).models.some(m => m.name === "Real Enter review"), "real_enter_no_unintended_save");
    await sleep(8000);
    await close();
    await theme("dark");
    await openEditor();
    await sleep(20000);
    await close();
  }
  for (const m of (await api.snapshot()).models) await api.remove(m.id);
  await api.clearRecords();
  await theme("light");
  await size(960, 640);
  await page("models");
  await refresh();
  return checks;
}
