// Grammable — the popup: action menu → working → review, or a message.
"use strict";

const tauri = window.__TAURI__;
const invoke = tauri.core.invoke;
const card = document.getElementById("card");
const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  for (const c of children) if (c !== null && c !== undefined && c !== false) node.append(c);
  return node;
};

let ctx = null;           // popup_context: presets, default backend, word count
let state = { phase: "hidden" };
let runToken = 0;         // ignores results from runs that were cancelled or replaced
let lastPreset = null;
let active = 0;           // highlighted menu row

const badge = (onDevice) => el("span", {
  className: `badge ${onDevice ? "device" : "cloud"}`,
  textContent: onDevice ? "🔒 On-device" : "☁ Cloud",
});

// MARK: Rendering

function render() {
  card.hidden = state.phase === "hidden";
  card.className = `card ${state.phase}`;
  card.replaceChildren();
  if (state.phase === "menu") renderMenu();
  else if (state.phase === "working") renderWorking();
  else if (state.phase === "review") renderReview();
  else if (state.phase === "message") renderMessage();
  // Measure now: a hidden window gets no animation frames, and the window is
  // only shown once Rust knows the size.
  if (state.phase !== "hidden") resize();
}

/** Tells Rust the card's size, so the window fits it (and is shown). */
function resize() {
  if (card.hidden) return;
  const r = card.getBoundingClientRect();
  invoke("popup_resize", { width: Math.ceil(r.width) + 12, height: Math.ceil(r.height) + 12 }).catch(console.error);
}

function renderMenu() {
  const words = ctx.wordCount;
  const rows = ctx.presets.map((p, i) => {
    const row = el("button", { className: `preset${i === active ? " active" : ""}`, title: p.onDevice ? "Runs on this computer" : "Sent to a cloud AI" },
      el("span", { className: "icon", textContent: presetIcon(p.icon) }),
      el("span", { className: "name", textContent: p.name }),
      p.backend !== ctx.defaultBackend && el("span", { className: "pill", textContent: p.backendName }),
      i < 9 && el("kbd", { textContent: String(i + 1) }));
    row.addEventListener("click", () => run(p));
    row.addEventListener("mouseenter", () => setActive(i));
    return row;
  });
  const gear = el("button", { className: "icon-btn", textContent: "⚙", title: "Presets & settings" });
  gear.addEventListener("click", () => invoke("popup_open_settings", { tab: "presets" }));
  card.append(
    el("header", { className: "row" },
      el("span", { className: "brand", textContent: "Grammable" }),
      el("span", { className: "muted small", textContent: `· ${words} word${words === 1 ? "" : "s"}` }),
      el("span", { className: "spacer" }),
      badge(ctx.defaultOnDevice)),
    el("hr"),
    el("div", { className: "presets" }, ...rows),
    el("hr"),
    el("footer", { className: "row" },
      el("span", { className: "muted tiny", textContent: `Using ${ctx.defaultBackendName}` }),
      el("span", { className: "spacer" }),
      gear));
}

function setActive(i) {
  active = i;
  card.querySelectorAll(".preset").forEach((row, j) => row.classList.toggle("active", j === i));
}

function renderWorking() {
  const p = state.preset;
  const cancel = el("button", { className: "btn small", textContent: "Cancel" });
  cancel.addEventListener("click", dismiss);
  card.append(el("div", { className: "row", style: "gap:10px" },
    el("div", { className: "spinner" }),
    el("div", {},
      el("div", { style: "font-weight:600;font-size:12px", textContent: p.name }),
      el("div", { className: "row muted small" }, `Asking ${p.backendName}… `, badge(p.onDevice))),
    el("span", { className: "spacer" }),
    cancel));
}

const changesOf = (r) => r.diff.segments.filter((s) => s.kind === "change").map((s) => s.change);

function finalText() {
  const r = state.result;
  if (r.mode !== "fix") return state.edited;
  return r.diff.segments.map((s) => s.kind === "same" ? s.text
    : state.accepted.has(s.change.id) ? s.change.replacement : s.change.original).join("");
}

function visible(s) {
  if (!s) return "∅";
  if (!s.trim()) return s.includes("\n") ? "↵ line break" : "␣ space";
  return s.trim();
}

function renderReview() {
  const r = state.result;
  const preset = ctx.presets.find((p) => p.id === r.presetId);
  const close = el("button", { className: "icon-btn", textContent: "✕", title: "Close (Esc)" });
  close.addEventListener("click", dismiss);
  card.append(el("div", { className: "row" },
    el("span", { textContent: presetIcon(preset?.icon) }),
    el("strong", { textContent: r.presetName }),
    badge(r.onDevice),
    el("span", { className: "muted tiny", textContent: `${(r.durationMs / 1000).toFixed(1)}s · ${r.backendName}` }),
    el("span", { className: "spacer" }),
    close));

  if (r.unchanged) {
    card.append(el("div", { className: "row good", style: "gap:10px" },
      el("span", { className: "mark", textContent: "✔" }),
      el("div", {}, el("strong", { textContent: "Looks good!" }), el("div", { className: "muted small", textContent: "No changes suggested." }))));
  } else if (r.mode === "fix") {
    renderFix(r);
  } else {
    renderRewrite(r);
  }

  const retry = el("button", { className: "btn", textContent: "↻ Retry", title: "Ask again for a different suggestion" });
  retry.addEventListener("click", () => lastPreset && run(lastPreset));
  const copy = el("button", { className: "btn", textContent: "Copy", title: "Copy (Ctrl+Shift+C)" });
  copy.addEventListener("click", doCopy);
  const main = el("button", { className: "btn primary", id: "primary" });
  if (r.unchanged) {
    main.textContent = "Done";
    main.addEventListener("click", dismiss);
  } else {
    main.textContent = "Replace";
    main.title = r.mode === "fix" ? "Replace (Enter)" : "Replace (Ctrl+Enter)";
    main.disabled = r.mode === "fix" && state.accepted.size === 0;
    main.addEventListener("click", doReplace);
  }
  card.append(el("div", { className: "row" }, retry, el("span", { className: "spacer" }), copy, main));
}

function renderFix(r) {
  const preview = el("div", { className: "preview" });
  for (const s of r.diff.segments) {
    if (s.kind === "same") { preview.append(s.text); continue; }
    const c = s.change;
    if (state.accepted.has(c.id)) {
      if (c.original) preview.append(el("del", { textContent: c.original }));
      if (c.original && c.replacement && !/\s$/.test(c.original)) preview.append(" ");
      if (c.replacement) preview.append(el("ins", { textContent: c.replacement }));
    } else {
      preview.append(el("u", { textContent: c.original }));
    }
  }
  const changes = changesOf(r);
  const all = el("button", { className: "link", textContent: "Accept all" });
  all.addEventListener("click", () => { state.accepted = new Set(changes.map((c) => c.id)); render(); });
  const none = el("button", { className: "link", textContent: "Reject all" });
  none.addEventListener("click", () => { state.accepted = new Set(); render(); });

  const rows = changes.map((c) => {
    const on = state.accepted.has(c.id);
    const insertion = !c.original && !!c.replacement;
    const deletion = !!c.original && !c.replacement;
    const row = el("button", { className: `change${on ? " on" : ""}` },
      el("span", { className: "check", textContent: on ? "●" : "○" }),
      !insertion && el("span", { className: "old", textContent: visible(c.original) }),
      !insertion && !deletion && el("span", { className: "muted tiny", textContent: "→" }),
      !deletion && el("span", { className: "new", textContent: visible(c.replacement) }),
      el("span", { className: "kind", textContent: insertion ? "add" : deletion ? "remove" : "replace" }));
    row.addEventListener("click", () => {
      if (state.accepted.has(c.id)) state.accepted.delete(c.id); else state.accepted.add(c.id);
      render();
    });
    return row;
  });
  card.append(preview,
    el("div", { className: "row" },
      el("span", { className: "muted small", style: "font-weight:600", textContent: `${state.accepted.size} of ${changes.length} changes` }),
      el("span", { className: "spacer" }), all, none),
    el("div", { className: "changes" }, ...rows));
}

function renderRewrite(r) {
  const edit = el("textarea", { className: "edit", value: state.edited, spellcheck: true });
  const fit = () => { edit.style.height = "auto"; edit.style.height = `${Math.min(260, Math.max(56, edit.scrollHeight + 2))}px`; };
  edit.addEventListener("input", () => { state.edited = edit.value; fit(); resize(); });
  const original = el("details", {}, el("summary", { textContent: "Original" }), el("div", { className: "original", textContent: r.original }));
  original.open = state.showOriginal;
  original.addEventListener("toggle", () => { state.showOriginal = original.open; resize(); });
  card.append(edit, original);
  fit();
  setTimeout(() => { edit.focus(); edit.setSelectionRange(0, 0); }, 0);
}

function renderMessage() {
  const close = el("button", { className: "btn primary small", textContent: "Close", id: "primary" });
  close.addEventListener("click", dismiss);
  const buttons = [el("span", { className: "spacer" }), close];
  if (state.isError) {
    const settings = el("button", { className: "btn small", textContent: "AI Settings…" });
    settings.addEventListener("click", () => invoke("popup_open_settings", { tab: "backends" }));
    buttons.unshift(settings);
    if (lastPreset) {
      const retry = el("button", { className: "btn small", textContent: "Retry" });
      retry.addEventListener("click", () => run(lastPreset));
      buttons.splice(1, 0, retry);
    }
  }
  card.append(
    el("div", { className: "row", style: "align-items:flex-start;gap:10px" },
      el("span", { className: "symbol", textContent: state.isError ? "⚠️" : "ℹ️" }),
      el("div", {},
        el("div", { style: "font-weight:600", textContent: state.title }),
        el("div", { className: "detail", textContent: state.detail }))),
    el("div", { className: "row" }, ...buttons));
}

// MARK: Actions

async function run(preset) {
  lastPreset = preset;
  state = { phase: "working", preset };
  render();
  const token = ++runToken;
  try {
    const result = await invoke("popup_run", { id: preset.id });
    if (token !== runToken) return;
    state = { phase: "review", result, accepted: new Set(changesOf(result).map((c) => c.id)), edited: result.suggestion, showOriginal: false };
  } catch (e) {
    if (token !== runToken || String(e) === "cancelled") return;
    state = { phase: "message", title: `${preset.backendName} couldn’t finish`, detail: String(e), isError: true };
  }
  render();
}

function dismiss() {
  runToken++;
  state = { phase: "hidden" };
  render();
  invoke("popup_dismiss");
}

function doReplace() {
  const text = finalText();
  runToken++;
  state = { phase: "hidden" };
  render();
  invoke("popup_replace", { text });
}

async function doCopy() {
  const text = finalText();
  runToken++;
  state = { phase: "hidden" };
  render();
  try { await invoke("popup_copy", { text }); } catch (e) { console.error(e); }
}

// MARK: Keyboard

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { e.preventDefault(); dismiss(); return; }
  if (state.phase === "menu") {
    const n = ctx.presets.length;
    if (/^[1-9]$/.test(e.key) && Number(e.key) <= n) { e.preventDefault(); run(ctx.presets[Number(e.key) - 1]); }
    else if (e.key === "ArrowDown" && n) { e.preventDefault(); setActive((active + 1) % n); }
    else if (e.key === "ArrowUp" && n) { e.preventDefault(); setActive((active - 1 + n) % n); }
    else if (e.key === "Enter" && n) { e.preventDefault(); run(ctx.presets[active]); }
    return;
  }
  if (state.phase === "review") {
    if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "c") { e.preventDefault(); doCopy(); return; }
    const inEditor = e.target.tagName === "TEXTAREA";
    if (e.key === "Enter" && (!inEditor || e.ctrlKey)) {
      const primary = document.getElementById("primary");
      if (primary && !primary.disabled) { e.preventDefault(); primary.click(); }
    }
    return;
  }
  if (state.phase === "message" && e.key === "Enter") { e.preventDefault(); dismiss(); }
});

// MARK: Events from Rust

async function show(view) {
  if (!view) return;
  ctx = await invoke("popup_context");
  active = 0;
  if (view.phase === "menu") {
    lastPreset = null;
    state = { phase: "menu" };
  } else {
    state = { phase: "message", title: view.title, detail: view.detail, isError: view.isError };
  }
  render();
}

tauri.event.listen("popup-open", (e) => show(e.payload));
tauri.event.listen("popup-closed", () => {
  runToken++;
  state = { phase: "hidden" };
  render();
});
// The window may load after the first trigger; pick up what is pending.
invoke("popup_context").then((c) => c.view && show(c.view));
