// Grammable — Settings window.
"use strict";

const tauri = window.__TAURI__;
const invoke = tauri.core.invoke;
const $ = (sel) => document.querySelector(sel);
const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  for (const c of children) node.append(c);
  return node;
};

let settings = null;
let info = null;
let backendInfo = [];

// MARK: Tabs

const TABS = ["general", "backends", "presets", "privacy", "about"];
const loaded = new Set();

function showTab(name) {
  if (!TABS.includes(name)) name = "general";
  for (const b of document.querySelectorAll("nav button")) b.classList.toggle("active", b.dataset.tab === name);
  for (const s of document.querySelectorAll("section.tab")) s.classList.toggle("active", s.id === `tab-${name}`);
  if (location.hash !== `#${name}`) history.replaceState(null, "", `#${name}`);
  if (name === "backends" && !loaded.has(name)) refreshBackends();
  if (name === "presets" && !loaded.has(name)) refreshPresets();
  loaded.add(name);
}

document.querySelector("nav").addEventListener("click", (e) => {
  const tab = e.target.closest("button")?.dataset.tab;
  if (tab) showTab(tab);
});
window.addEventListener("hashchange", () => showTab(location.hash.slice(1)));

// MARK: Saving

let saveTimer = null;
async function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try {
      await invoke("save_settings", { settings });
      $("#general-error").textContent = "";
    } catch (e) {
      $("#general-error").textContent = String(e);
    }
  }, 150);
}

function renderGeneral() {
  $("#onboarding").hidden = settings.hasCompletedOnboarding;
  $("#onboarding-hotkey").textContent = settings.hotkey;
  $("#hotkey-display").textContent = settings.hotkey;
  for (const input of document.querySelectorAll("[data-setting]")) {
    const key = input.dataset.setting;
    if (input.type === "checkbox") input.checked = !!settings[key];
    else input.value = settings[key];
  }
  const button = document.querySelector('[data-setting="popupOnSelection"]');
  button.disabled = !settings.isEnabled || info?.session === "wayland";
}

for (const input of document.querySelectorAll("[data-setting]")) {
  input.addEventListener("change", () => {
    const key = input.dataset.setting;
    if (input.type === "checkbox") settings[key] = input.checked;
    else {
      const n = Math.round(Number(input.value));
      settings[key] = Math.min(200, Math.max(1, Number.isFinite(n) ? n : 3));
      input.value = settings[key];
    }
    renderGeneral();
    save();
  });
}

$("#onboarding-done").addEventListener("click", () => {
  settings.hasCompletedOnboarding = true;
  renderGeneral();
  save();
});

$("#launch-at-login").addEventListener("change", async (e) => {
  try {
    e.target.checked = await invoke("set_launch_at_login", { enabled: e.target.checked });
    $("#general-error").textContent = "";
  } catch (err) {
    e.target.checked = !e.target.checked;
    $("#general-error").textContent = `Launch at login: ${err}`;
  }
});

// MARK: AI Backends

function badge(onDevice) {
  return el("span", {
    className: `badge ${onDevice ? "device" : "cloud"}`,
    textContent: onDevice ? "🔒 On-device" : "☁ Cloud",
  });
}

const MODEL_HINTS = {
  claude: "haiku, sonnet", codex: "gpt-5-mini", gemini: "gemini-2.5-flash",
  ollama: "First installed model", "openai-compatible": "First loaded model",
};

function renderDefaultBackend() {
  const select = $("#default-backend");
  select.replaceChildren(...backendInfo.map((b) =>
    el("option", { value: b.id, textContent: `${b.available ? "✓ " : "   "}${b.name}`, selected: b.id === settings.defaultBackend })));
  if (!backendInfo.length) select.append(el("option", { value: settings.defaultBackend, textContent: settings.defaultBackend }));
}

$("#default-backend").addEventListener("change", (e) => {
  settings.defaultBackend = e.target.value;
  save();
});

function textField(value, placeholder, onChange) {
  const input = el("input", { type: "text", value: value ?? "", placeholder });
  input.addEventListener("change", () => onChange(input.value.trim()));
  return input;
}

function setMapValue(map, id, value) {
  if (value) map[id] = value; else delete map[id];
}

function renderBackends() {
  const list = $("#backend-list");
  list.replaceChildren();
  for (const b of backendInfo) {
    const result = el("div", { className: "test-result" });
    const test = el("button", { className: "btn small", textContent: "Test" });
    test.addEventListener("click", async () => {
      test.disabled = true;
      result.className = "test-result muted";
      result.textContent = "Testing…";
      try {
        const r = await invoke("test_backend", { id: b.id });
        result.className = "test-result";
        result.textContent = `✓ ${(r.durationMs / 1000).toFixed(1)}s — ${r.suggestion}`;
      } catch (err) {
        result.className = "test-result error-text";
        result.textContent = `✗ ${err}`;
      } finally {
        test.disabled = false;
      }
    });
    const help = el("a", { href: "#", textContent: "?", title: b.setupHint });
    help.addEventListener("click", (e) => { e.preventDefault(); invoke("open_url", { url: b.setupUrl }); });

    const fields = el("div", { className: "fields" });
    if (b.isCli) {
      fields.append(el("label", { textContent: "Path" }),
        textField(settings.backends.executablePaths?.[b.id], "Auto-detect", (v) => {
          settings.backends.executablePaths ??= {}; setMapValue(settings.backends.executablePaths, b.id, v); save();
        }));
    }
    if (b.id === "ollama" || b.id === "openai-compatible") {
      const key = b.id === "ollama" ? "ollamaUrl" : "openaiBaseUrl";
      fields.append(el("label", { textContent: "Server URL" }),
        textField(settings.backends[key], "", (v) => { settings.backends[key] = v; save(); }));
    }
    if (b.id !== "builtin") {
      let model;
      if (b.models?.length) {
        model = el("select");
        const current = settings.backends.models?.[b.id] ?? "";
        model.append(el("option", { value: "", textContent: MODEL_HINTS[b.id] ?? "Default" }));
        for (const m of b.models) model.append(el("option", { value: m, textContent: m, selected: m === current }));
        if (current && !b.models.includes(current)) model.append(el("option", { value: current, textContent: current, selected: true }));
        model.addEventListener("change", () => { settings.backends.models ??= {}; setMapValue(settings.backends.models, b.id, model.value); save(); });
      } else {
        model = textField(settings.backends.models?.[b.id], MODEL_HINTS[b.id] ?? "Default", (v) => {
          settings.backends.models ??= {}; setMapValue(settings.backends.models, b.id, v); save();
        });
      }
      fields.append(el("label", { textContent: "Model" }), model);
    }

    list.append(el("div", { className: "backend" },
      el("header", {},
        el("span", { className: `dot ${b.available ? "ok" : "missing"}` }),
        el("strong", { textContent: b.name, title: b.privacyNote }),
        badge(b.onDevice), help, test),
      el("div", { className: "detail", textContent: b.detail }),
      fields.childElementCount ? fields : "",
      result));
  }
}

async function refreshBackends() {
  $("#refresh-backends").disabled = true;
  try {
    backendInfo = await invoke("backend_statuses");
    renderDefaultBackend();
    renderBackends();
  } catch (e) {
    $("#backend-list").textContent = String(e);
  } finally {
    $("#refresh-backends").disabled = false;
  }
}
$("#refresh-backends").addEventListener("click", refreshBackends);

// MARK: Presets

async function refreshPresets() {
  const list = await invoke("load_presets");
  const names = Object.fromEntries(backendInfo.map((b) => [b.id, b.shortName]));
  $("#preset-list").replaceChildren(...list.presets.map((p) => {
    const kind = p.isBuiltIn ? "built-in" : p.customized ? "customized" : "your preset";
    const action = el("button", { className: "btn small", textContent: p.isBuiltIn ? "Customize" : "Edit" });
    action.addEventListener("click", () => run(p.isBuiltIn ? "customize_preset" : "edit_preset", { id: p.id }));
    return el("div", { className: "preset" },
      el("span", { className: "icon", textContent: presetIcon(p.icon) }),
      el("div", { className: "label", textContent: p.name },
        el("small", { textContent: `${p.mode === "fix" ? "Accept / reject changes" : "Before / after"} · ${names[p.resolvedBackend] ?? p.resolvedBackend} · ${kind}` })),
      action);
  }));
  $("#preset-errors").textContent = list.errors.join("\n");
  $("#presets-dir").textContent = `Presets folder: ${list.directory}`;
}

async function run(command, args) {
  try {
    await invoke(command, args);
    $("#preset-errors").textContent = "";
  } catch (e) {
    $("#preset-errors").textContent = String(e);
  }
}

$("#create-preset").addEventListener("click", async () => {
  const name = $("#new-preset-name").value.trim();
  if (!name) return $("#new-preset-name").focus();
  await run("create_preset", { name });
  $("#new-preset-name").value = "";
  refreshPresets();
});
$("#open-presets-folder").addEventListener("click", () => run("open_presets_folder"));
$("#reload-presets").addEventListener("click", refreshPresets);
window.addEventListener("focus", () => { if (loaded.has("presets")) refreshPresets(); });

// MARK: Privacy

let defaultExcluded = [];
function renderExcluded() {
  $("#excluded-list").replaceChildren(...settings.excludedApps.map((name, i) => {
    const remove = el("button", { className: "btn small", textContent: "Remove" });
    remove.addEventListener("click", () => { settings.excludedApps.splice(i, 1); renderExcluded(); save(); });
    return el("div", { className: "excluded" }, el("code", { textContent: name }), remove);
  }));
  if (!settings.excludedApps.length) $("#excluded-list").append(el("p", { className: "muted", textContent: "No excluded apps." }));
}
$("#add-excluded").addEventListener("click", () => {
  const name = $("#new-excluded").value.trim().replace(/\.exe$/i, "");
  if (!name) return;
  if (!settings.excludedApps.some((a) => a.toLowerCase() === name.toLowerCase())) settings.excludedApps.push(name);
  $("#new-excluded").value = "";
  renderExcluded();
  save();
});
$("#restore-excluded").addEventListener("click", () => {
  settings.excludedApps = [...defaultExcluded];
  renderExcluded();
  save();
});

// MARK: Links

document.addEventListener("click", (e) => {
  const a = e.target.closest("[data-url], [data-url-key]");
  if (!a) return;
  e.preventDefault();
  const url = a.dataset.url ?? (a.dataset.urlKey === "issues" ? `${info.repository}/issues` : info[a.dataset.urlKey]);
  if (url) invoke("open_url", { url });
});

// MARK: Start

async function start() {
  [settings, info, defaultExcluded] = await Promise.all([
    invoke("get_settings"), invoke("app_info"), invoke("default_excluded_apps"),
  ]);
  $("#launch-at-login").checked = await invoke("get_launch_at_login");
  $("#about-version").textContent = `Version ${info.version} · ${info.platform}${info.session !== info.platform ? ` (${info.session})` : ""}`;
  $("#about-paths").textContent = `Settings: ${info.settingsPath}`;
  renderGeneral();
  renderDefaultBackend();
  renderExcluded();
  showTab(location.hash.slice(1));

  // Changes made elsewhere (tray menu) and requests to open a tab.
  await tauri.event.listen("settings-changed", (e) => {
    settings = e.payload;
    renderGeneral();
    renderDefaultBackend();
    renderExcluded();
  });
  await tauri.event.listen("open-tab", (e) => showTab(e.payload));
}

start().catch((e) => { document.body.prepend(el("p", { className: "error-text", textContent: String(e) })); });
