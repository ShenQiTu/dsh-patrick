import { invoke } from "@tauri-apps/api/core";
import { api } from "./api";
import { emptyModel, type Protocol } from "./types";
import { getCurrentWindow } from "@tauri-apps/api/window";

export async function runSmoke() {
  const checks: string[] = [];
  const assert = (condition: unknown, name: string) => {
    if (!condition) throw new Error(name);
    checks.push(name);
  };
  const wait = async (selector: string) => {
    for (let i = 0; i < 100; i++) {
      const e = document.querySelector<HTMLElement>(selector);
      if (e) return e;
      await new Promise((r) => setTimeout(r, 30));
    }
    throw new Error("DOM missing: " + selector);
  };
  const until = async (
    predicate: () => boolean | Promise<boolean>,
    name: string,
  ) => {
    for (let i = 0; i < 500; i++) {
      if (await predicate()) return;
      await new Promise((r) => setTimeout(r, 20));
    }
    throw new Error("Timed out: " + name);
  };
  try {
    await wait("[data-add-model]");
    assert(
      document.title === `DSH Patrick v${(await api.snapshot()).appVersion}`,
      "document_title_version",
    );
    assert(
      innerWidth <= 1000 && document.documentElement.scrollWidth <= innerWidth,
      "compact_window_no_horizontal_overflow",
    );
    assert(
      document.querySelector("[data-dsh-status]")?.textContent === "未接管",
      "sidebar_real_dsh_status",
    );
    assert(document.querySelector("img.icon-dsh"), "sidebar_dsh_asset");
    await until(() => {
      const logo = document.querySelector<HTMLImageElement>(
        ".compact-brand .app-logo",
      );
      return (
        !!logo?.complete &&
        logo.naturalWidth === 128 &&
        logo.naturalHeight === 128
      );
    }, "Patrick app logo loaded");
    assert(
      document.querySelector<HTMLImageElement>(".app-logo")?.alt ===
        "DSH Patrick 图标",
      "patrick_brand_icon_loaded",
    );
    assert(
      document
        .querySelector<HTMLLinkElement>('link[rel="icon"]')
        ?.href.includes("patrick-icon"),
      "patrick_favicon_configured",
    );
    assert(
      document.body.textContent?.includes("DSH Patrick"),
      "brand_rendered",
    );
    assert(!document.querySelector("textarea"), "prompt_hidden_on_home");
    const window = getCurrentWindow();
    assert(!(await window.isDecorated()), "native_titlebar_removed");
    assert(
      getComputedStyle(document.body).fontSize === "14px",
      "reference_body_font_14px",
    );
    assert(
      getComputedStyle(await wait(".sidebar nav button")).fontSize === "14px",
      "reference_navigation_font_14px",
    );
    assert(
      (await wait(".window-header")).getBoundingClientRect().height === 30,
      "reference_header_height_30px",
    );
    (await wait('[aria-label="最大化窗口"]')).click();
    await until(() => window.isMaximized(), "maximize window");
    (await wait('[aria-label="还原窗口"]')).click();
    await until(async () => !(await window.isMaximized()), "restore window");
    await until(() => innerWidth === 960 && innerHeight === 640, "restored viewport settled");
    assert(true, "custom_maximize_restore_controls");
    (await wait('[aria-label="最小化窗口"]')).click();
    await until(() => window.isMinimized(), "minimize window");
    await window.unminimize();
    await until(async () => !(await window.isMinimized()), "unminimize window");
    assert(true, "custom_minimize_control");
    (await wait("[data-add-model]")).click();
    await wait("dialog[open]");
    const context = (await wait(
      'input[name="contextWindow"]',
    )) as HTMLInputElement;
    const output = (await wait('input[name="maxOutput"]')) as HTMLInputElement;
    assert(
      context.value === "" &&
        output.value === "" &&
        !context.required &&
        !output.required,
      "new_model_token_inputs_empty_optional",
    );
    assert(
      ((await wait('input[name="baseUrl"]')) as HTMLInputElement)
        .placeholder === "https://api.openai.com",
      "reference_base_url_placeholder",
    );
    assert(
      getComputedStyle(context).fontSize === "13px" &&
        getComputedStyle(context.closest(".field")!).fontSize === "13px",
      "reference_field_input_font_13px",
    );
    assert(
      context.getBoundingClientRect().height === 34,
      "reference_input_height_34px",
    );
    assert(
      context.getBoundingClientRect().top ===
        output.getBoundingClientRect().top,
      "token_fields_aligned",
    );
    const modal = document.querySelector<HTMLDialogElement>("dialog[open]")!;
    assert(
      modal.matches(":modal") &&
        getComputedStyle(modal, "::backdrop").backgroundColor ===
          "rgba(0, 0, 0, 0.533)",
      "modal_top_layer_covers_custom_header",
    );
    assert(
      modal.scrollWidth <= modal.clientWidth &&
        (await wait(".modal-foot")).getBoundingClientRect().bottom <=
          innerHeight,
      "editor_no_horizontal_overflow_footer_visible",
    );
    const editorScroll = await wait(".editor-scroll");
    editorScroll.scrollTop = editorScroll.scrollHeight;
    assert(editorScroll.scrollTop > 0, "editor_body_scrolls_in_compact_window");
    assert(
      (await wait('[aria-label="Zen 请求头（仅测试）"]')).getAttribute("aria-checked") === "false",
      "zen_test_switch_defaults_off",
    );
    const enabledToggle = await wait('[aria-label="启用模型"]');
    assert(
      enabledToggle.getBoundingClientRect().bottom <=
        editorScroll.getBoundingClientRect().bottom,
      "editor_last_controls_reachable",
    );
    editorScroll.scrollTop = 0;
    assert(
      document.querySelector('input[name="apiKey"]')?.getAttribute("type") ===
        "password",
      "key_input_masked",
    );
    (await wait('[aria-label="关闭弹窗"]')).click();
    const draft = {
      ...emptyModel(),
      name: "Native smoke model",
      model: "test-only-model",
      baseUrl: "https://example.invalid/v1",
      apiKey: "TEST_ONLY_SECRET",
      headers: { "x-test": "native" },
      prompt: "TEST_ONLY_PROMPT",
    };
    const id = await api.save(draft);
    let state = await api.snapshot();
    assert(
      state.models[0].contextWindow === null &&
        state.models[0].maxOutput === null,
      "blank_token_values_saved_as_null",
    );
    assert(
      state.models.length === 1 && state.models[0].hasKey,
      "model_created",
    );
    assert(
      state.models[0].apiKey === "" &&
        !JSON.stringify(state).includes("TEST_ONLY_SECRET"),
      "key_not_returned",
    );
    await api.save({
      ...state.models[0],
      name: "Edited native model",
      apiKey: null,
    });
    state = await api.snapshot();
    assert(
      state.models[0].name === "Edited native model" && state.models[0].hasKey,
      "edit_preserves_key",
    );
    let changedOrigin = false;
    try {
      await api.save({
        ...state.models[0],
        baseUrl: "https://other.invalid/v1",
        apiKey: null,
      });
    } catch {
      changedOrigin = true;
    }
    assert(changedOrigin, "origin_change_requires_new_key");
    const copied = await api.duplicate(id);
    state = await api.snapshot();
    assert(
      state.models.length === 2 && state.models[1].hasKey,
      "duplicate_preserves_key",
    );
    let rejected = false;
    try {
      await api.save({ ...draft, id, headers: { Authorization: "forbidden" } });
    } catch {
      rejected = true;
    }
    assert(rejected, "reserved_header_rejected");
    rejected = false;
    try {
      await api.save({ ...draft, id, baseUrl: "http://example.invalid/v1" });
    } catch {
      rejected = true;
    }
    assert(rejected, "remote_plain_http_rejected");
    await api.settings({
      ...state.settings,
      theme: "dark",
      timeoutSeconds: 60,
      maxBodyMb: 32,
    });
    state = await api.snapshot();
    assert(
      state.settings.theme === "dark" &&
        state.settings.timeoutSeconds === 60 &&
        state.settings.maxBodyMb === 32,
      "settings_persisted",
    );
    await api.remove(copied);
    await api.remove(id);
    state = await api.snapshot();
    assert(state.models.length === 0, "models_deleted");
    await api.settings({
      ...state.settings,
      theme: "light",
      timeoutSeconds: 120,
      maxBodyMb: 64,
    });
    const upstream = state.testUpstream;
    assert(
      upstream?.startsWith("http://127.0.0.1:"),
      "native_mock_is_loopback",
    );
    const ids: string[] = [];
    for (const protocol of ["openai", "responses", "anthropic"] as Protocol[]) {
      const next = await api.save({
        ...emptyModel(),
        name: "Mock " + protocol,
        model: protocol === "responses" ? "native-zen-mock" : "native-mock",
        zenTestEnabled: protocol === "responses",
        protocol,
        baseUrl: upstream!,
        apiKey: "NATIVE_MOCK_KEY",
        prompt: "MOCK_SYSTEM_PROMPT",
        promptEnabled: true,
      });
      ids.push(next);
      const result = await api.test(next);
      assert(
        result.state === "success" &&
          result.outputTokens === 8 &&
          result.preview === "Hello from native mock",
        "native_test_" + protocol,
      );
    }
    const port = await api.start();
    assert(port > 0 && port !== 8787, "gateway_random_port_started");
    assert((await api.snapshot()).trayAvailable, "native_tray_created");
    await invoke("smoke_lifecycle", { action: "show" });
    await until(() => window.isVisible(), "window visible before close");
    (await wait('[aria-label="关闭窗口"]')).click();
    await until(async () => !(await window.isVisible()), "close hides to tray");
    assert((await api.snapshot()).gatewayRunning, "tray_hide_preserves_gateway");
    assert(
      (await invoke("smoke_gateway_probe", { id: ids[0] })) ===
        "NATIVE_GATEWAY_GZIP_PROXY_PASS",
      "native_gzip_proxy",
    );
    await invoke("smoke_lifecycle", { action: "show" });
    await until(() => window.isVisible(), "tray restores window");
    assert(true, "tray_restore_window");
    await api.stop();
    state = await api.snapshot();
    assert(
      !state.gatewayRunning && state.runtime.activeRequests === 0,
      "gateway_stopped",
    );
    assert(state.runtime.calls.length === 4, "call_records_integrated");
    // Drive the actual React single/batch handlers, rather than substituting IPC calls.
    (await wait('[data-nav="models"]')).click();
    (await wait('[aria-label="刷新"]')).click();
    (await wait(`[data-test-model="${ids[1]}"]`)).click();
    (await wait("[data-confirm-test]")).click();
    await until(async () => {
      const s = await api.snapshot();
      // Snapshot counters and records are sampled separately; a new record can
      // appear before the active count is observed. Wait for a terminal record.
      return s.runtime.calls.length === 5 && s.runtime.activeRequests === 0 &&
        s.runtime.calls[0].state !== "running";
    }, "single UI test");
    assert(
      (await api.snapshot()).runtime.calls[0].state === "success",
      "single_test_ui_flow",
    );
    assert((await api.snapshot()).runtime.calls[0].zenHeaderSent === true, "single_zen_test_reports_header_sent");
    for (let i = 0; i < 2; i++) {
      ids.push(
        await api.save({
          ...emptyModel(),
          name: `Batch extra ${i}`,
          model: "native-mock",
          baseUrl: upstream!,
          apiKey: "NATIVE_MOCK_KEY",
        }),
      );
    }
    (await wait('[aria-label="刷新"]')).click();
    await until(
      () => document.querySelectorAll("[data-test-model]").length === 5,
      "five model cards",
    );
    (await wait("[data-batch-test]:not([disabled])")).click();
    (await wait("[data-confirm-test]")).click();
    await until(async () => {
      const s = await api.snapshot();
      return (
        s.runtime.calls.length === 10 &&
        s.runtime.activeRequests === 0 &&
        s.runtime.calls.slice(0, 5).every((r) => r.state !== "running") &&
        document
          .querySelector("[data-batch-test]")
          ?.textContent?.includes("一键测试") === true
      );
    }, "batch UI success");
    assert(
      Object.values((await api.snapshot()).runtime.testResults).every(
        (r) => r.state === "success",
      ),
      "batch_test_ui_five_models",
    );
    if (state.testPauseSeconds)
      await new Promise((resolve) =>
        setTimeout(resolve, Math.min(20, state.testPauseSeconds!) * 1000),
      );
    (await wait("button.test-status.success")).click();
    await wait(".response-preview");
    assert((await wait("[data-zen-status]")).textContent?.includes("本机测试"), "call_details_zen_presence_without_header_value");
    assert(
      document
        .querySelector(".response-preview")
        ?.textContent?.includes("Hello from native mock"),
      "test_details_rendered",
    );
    (await wait('[aria-label="关闭弹窗"]')).click();
    for (const model of (await api.snapshot()).models) {
      await api.save({ ...model, model: "native-slow", apiKey: null });
    }
    (await wait('[aria-label="刷新"]')).click();
    (await wait("[data-batch-test]:not([disabled])")).click();
    (await wait("[data-confirm-test]")).click();
    await until(
      async () => (await api.snapshot()).runtime.activeRequests === 3,
      "three active UI workers",
    );
    await new Promise((r) => setTimeout(r, 150));
    (await wait('[aria-label="关闭窗口"]')).click();
    await until(async () => !(await window.isVisible()), "active request close hides");
    assert((await api.snapshot()).runtime.activeRequests > 0, "tray_hide_preserves_active_requests");
    let exitBlocked = false;
    try { await invoke("smoke_lifecycle", { action: "exit-check" }); } catch { exitBlocked = true; }
    assert(exitBlocked, "tray_exit_blocks_active_requests");
    await invoke("smoke_lifecycle", { action: "show" });
    (await wait("[data-batch-test]")).click();
    await until(async () => {
      const s = await api.snapshot();
      return (
        s.runtime.activeRequests === 0 &&
        document
          .querySelector("[data-batch-test]")
          ?.textContent?.includes("一键测试") === true
      );
    }, "batch UI cancelled");
    state = await api.snapshot();
    assert(
      state.runtime.calls.length === 13 &&
        state.runtime.calls.slice(0, 3).every((r) => r.state === "cancelled"),
      "batch_cancel_ui_skips_two_queued_models",
    );
    assert(
      document.querySelectorAll("button.test-status.cancelled").length === 3,
      "cancelled_cards_rendered",
    );
    (await wait('[data-nav="calls"]')).click();
    await wait(".call-row");
    assert(
      document.querySelectorAll(".call-row").length === 13,
      "call_records_rendered",
    );
    const editModel = (await api.snapshot()).models[0];
    await api.save({
      ...editModel,
      model: "native-mock",
      apiKey: null,
      baseUrl: upstream! + "/chat/completions",
      useFullUrl: true,
    });
    (await wait('[data-nav="models"]')).click();
    (await wait('[aria-label="刷新"]')).click();
    await wait(`[data-model-url="${upstream}/chat/completions"]`);
    (await wait(`[aria-label="编辑 ${editModel.name}"]`)).click();
    (await wait("[data-discover-models]")).click();
    await wait('[role="option"]');
    assert(
      document.querySelectorAll('[role="option"]').length === 2,
      "discover_models_ui_list",
    );
    (await wait('[role="option"]')).click();
    const setInput = async (selector: string, value: string) => {
      const element = await wait(selector);
      const prototype =
        element instanceof HTMLTextAreaElement
          ? HTMLTextAreaElement.prototype
          : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(
        element,
        value,
      );
      element.dispatchEvent(new Event("input", { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 50));
    };
    await setInput('input[name="notes"]', "Native UI note");
    (await wait('[role="switch"][aria-label="Zen 请求头（仅测试）"]')).click();
    await until(() => document.querySelector('[aria-label="Zen 请求头（仅测试）"]')?.getAttribute("aria-checked") === "true", "zen toggle on");
    (await wait('[role="switch"][aria-label="额外参数"]')).click();
    await setInput(
      'textarea[aria-label="额外参数 JSON"]',
      '{"temperature":0.25}',
    );
    if (state.testPauseSeconds)
      await new Promise((resolve) =>
        setTimeout(resolve, Math.min(20, state.testPauseSeconds!) * 1000),
      );
    (await wait("[data-save-test]")).click();
    await until(async () => {
      const s = await api.snapshot();
      return (
        s.runtime.calls.length === 14 &&
        s.runtime.activeRequests === 0 &&
        s.runtime.calls[0].state !== "running" &&
        !document.querySelector("dialog[open]")
      );
    }, "save and test UI flow");
    state = await api.snapshot();
    assert(
      state.runtime.calls[0].state === "success" &&
        state.models[0].useFullUrl &&
        state.models[0].notes === "Native UI note" &&
        state.models[0].extraParams.temperature === 0.25,
      "save_and_test_full_url_notes_extra_ui",
    );
    assert(state.models[0].zenTestEnabled, "zen_test_flag_saved_through_ui_and_native_api");
    const toastCheck = await wait(".toast > img.icon-check");
    assert(getComputedStyle(toastCheck).filter === "brightness(0) invert(1)", "toast_check_is_white_scoped_to_toast");
    (await wait(`[aria-label="编辑 ${editModel.name}"]`)).click();
    assert((await wait('[aria-label="Zen 请求头（仅测试）"]')).getAttribute("aria-checked") === "true", "zen_switch_restored_when_editor_reopened");
    (await wait('[aria-label="Zen 请求头（仅测试）"]')).click();
    await until(() => document.querySelector('[aria-label="Zen 请求头（仅测试）"]')?.getAttribute("aria-checked") === "false", "zen toggle off");
    (document.querySelector("form.compact-editor") as HTMLFormElement).requestSubmit();
    await until(() => !document.querySelector("dialog[open]"), "save disabled Zen switch");
    assert(!(await api.snapshot()).models[0].zenTestEnabled, "zen_switch_disabled_and_saved");
    assert(
      document.querySelector(".model-notes")?.textContent === "Native UI note",
      "model_note_on_card",
    );
    // Exercise the real test button + IPC + HTTP stream + visible terminal state.
    const diagnosticSettings = (await api.snapshot()).settings;
    await api.settings({ ...diagnosticSettings, timeoutSeconds: 5 });
    for (const [model, expected, reason] of [
      ["native-diag-error", "error", "Access forbidden [已脱敏]"],
      ["native-diag-empty", "error", "空响应"],
      ["native-diag-truncated", "error", "缺少模型结束事件"],
      ["native-diag-timeout", "error", "读取上游响应超时"],
      ["native-mock", "success", "模型连接测试通过"],
    ]) {
      const id = await api.save({ ...emptyModel(), name: "Zen result diagnostic", model,
        baseUrl: upstream!, apiKey: "NATIVE_MOCK_KEY", zenTestEnabled: true });
      (await wait('[aria-label="刷新"]')).click();
      (await wait(`[data-test-model="${id}"]`)).click();
      (await wait("[data-confirm-test]")).click();
      await until(async () => {
        const s = await api.snapshot();
        const button = document.querySelector<HTMLElement>(`[data-test-model="${id}"]`);
        return s.runtime.testResults[id]?.state === expected && button?.textContent === "测试" &&
          !!button.closest(".model-card")?.querySelector(`button.test-status.${expected}`) &&
          !button.closest(".model-card")?.querySelector(".test-status.running");
      }, `${model} completes and resets loading`);
      assert(true, `${model}_button_loading_ends`);
      const button = await wait(`[data-test-model="${id}"]`);
      button.closest(".model-card")!.querySelector<HTMLButtonElement>(`button.test-status.${expected}`)!.click();
      const banner = await wait(`.result-banner.${expected}`);
      assert(banner.textContent?.includes(reason), `${model}_visible_terminal_reason`);
      assert(!document.querySelector(".detail-body")?.textContent?.includes("NATIVE_MOCK_KEY"), `${model}_details_redacted`);
      (await wait('[aria-label="关闭弹窗"]')).click();
      await api.remove(id);
    }
    await api.settings(diagnosticSettings);
    (await wait('[data-nav="help"]')).click();
    await wait(".help-panel");
    assert(document.body.textContent?.includes("获取模型"), "local_help_page");
    for (const id of ids) await api.remove(id);
    await api.clearRecords();
    (await wait('[aria-label="刷新"]')).click();
    (await wait('[data-nav="settings"]')).click();
    await wait('[role="combobox"][aria-label="界面主题"]');
    assert(
      ((await wait('input[name="maxBodyMb"]')) as HTMLInputElement).value ===
        "64",
      "request_limit_setting_rendered",
    );
    assert(
      document.body.textContent?.includes("Windows 当前用户 DPAPI 加密"),
      "settings_page_rendered",
    );
    await until(() => {
      const logo = document.querySelector<HTMLImageElement>(
        ".about-panel .app-logo",
      );
      return !!logo?.complete && logo.naturalWidth === 128;
    }, "about Patrick image loaded");
    assert(
      !document.querySelector(".about-panel .brand-symbol"),
      "about_uses_patrick_whale_not_placeholder",
    );
    assert(
      !state.gatewayRunning && !state.dshConnected,
      "dsh_untouched_gateway_stopped",
    );
    const { runUIRegression } = await import("./uiRegression");
    checks.push(...(await runUIRegression()));
    const { runDesktopRegression } = await import("./desktopRegression");
    checks.push(...(await runDesktopRegression()));
    await invoke("smoke_finish", { checks });
    (await wait('[aria-label="关闭窗口"]')).click();
  } catch (error) {
    console.error("UI_SMOKE_FAILURE", String(error));
    await invoke("smoke_fail", { message: String(error) });
  }
}
