import { basicSetup, EditorView } from "codemirror";
import { EditorState } from "@codemirror/state";
import { setDiagnostics } from "@codemirror/lint";
import { typstLanguage } from "./typst-language.js";

const boot = JSON.parse(document.getElementById("boot").textContent);
const storageKey = `typst-letter:${boot.slug}`;

// ---------- DOM scaffold ----------

const app = document.getElementById("app");
app.innerHTML = `
  <header class="toolbar">
    <a href="/" class="home">&larr; ${boot.slug}</a>
    <span id="status" class="status"></span>
    <button id="toggle" class="toggle" hidden>Preview</button>
    <button id="download">Download PDF</button>
  </header>
  <main class="split">
    <section id="editor-pane" class="pane">
      <div id="editor"></div>
      <div id="diagnostics" class="diagnostics" hidden></div>
    </section>
    <div id="divider" class="divider" role="separator" aria-orientation="vertical"></div>
    <section id="preview-pane" class="pane preview"><div id="preview-empty">compiling&hellip;</div></section>
  </main>
`;

const statusEl = document.getElementById("status");
const diagsEl = document.getElementById("diagnostics");
const previewPane = document.getElementById("preview-pane");
const splitEl = document.querySelector(".split");

// ---------- Editor ----------

const stored = localStorage.getItem(storageKey);
const initialSource = stored !== null && stored !== boot.source ? stored : boot.source;

const view = new EditorView({
  parent: document.getElementById("editor"),
  state: EditorState.create({
    doc: initialSource,
    extensions: [
      basicSetup,
      typstLanguage,
      EditorView.lineWrapping,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) onEdit();
      }),
    ],
  }),
});

// ---------- Compile pipeline ----------

let debounceTimer = null;
let inFlight = null; // AbortController
let seq = 0; // sequence of the last *started* request
let applied = 0; // sequence of the last response applied to the preview
let currentBlobUrl = null;
let retried429 = false;

function onEdit() {
  localStorage.setItem(storageKey, view.state.doc.toString());
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(compile, boot.debounceMs);
}

async function compile() {
  const mySeq = ++seq;
  if (inFlight) inFlight.abort();
  const controller = new AbortController();
  inFlight = controller;
  setStatus("compiling…");
  let res;
  try {
    res = await fetch(`/${boot.slug}/compile`, {
      method: "POST",
      headers: { "Content-Type": "text/plain; charset=utf-8" },
      body: view.state.doc.toString(),
      signal: controller.signal,
    });
  } catch (e) {
    if (controller.signal.aborted) return; // superseded by a newer edit
    setStatus("network error");
    return;
  }
  if (mySeq <= applied) return; // an even newer response already landed

  if (res.status === 429 && !retried429) {
    retried429 = true;
    setTimeout(() => {
      if (mySeq === seq) compile(); // still the newest state → retry once
    }, 500);
    return;
  }
  retried429 = false;

  if (res.ok) {
    const blob = await res.blob();
    if (mySeq <= applied) return;
    applied = mySeq;
    showPdf(blob);
    const warnings = parseWarningsHeader(res.headers.get("x-typst-warnings"));
    renderDiagnostics(warnings);
    setStatus("");
  } else if (res.status === 422) {
    const diags = await res.json().catch(() => []);
    if (mySeq <= applied) return;
    applied = mySeq;
    renderDiagnostics(diags); // keeps last good PDF
    setStatus("error");
  } else {
    setStatus(`compile failed (${res.status})`);
  }
}

function parseWarningsHeader(raw) {
  if (!raw) return [];
  try {
    return JSON.parse(raw);
  } catch {
    return [];
  }
}

function showPdf(blob) {
  const url = URL.createObjectURL(blob);
  // Recreate the <embed>: some engines ignore src changes on live embeds.
  const embed = document.createElement("embed");
  embed.type = "application/pdf";
  embed.src = url;
  previewPane.replaceChildren(embed);
  if (currentBlobUrl) URL.revokeObjectURL(currentBlobUrl);
  currentBlobUrl = url;
}

function setStatus(text) {
  statusEl.textContent = text;
}

// ---------- Diagnostics ----------

function renderDiagnostics(diags) {
  diagsEl.hidden = diags.length === 0;
  diagsEl.replaceChildren(
    ...diags.map((d) => {
      const row = document.createElement("div");
      row.className = `diag ${d.severity}`;
      const loc = d.line ? `${d.line}:${d.col ?? 1} ` : "";
      row.textContent = `${d.severity}: ${loc}${d.message}`;
      if (d.line) {
        row.style.cursor = "pointer";
        row.onclick = () => {
          const line = view.state.doc.line(Math.min(d.line, view.state.doc.lines));
          view.dispatch({ selection: { anchor: line.from }, scrollIntoView: true });
          view.focus();
        };
      }
      return row;
    })
  );

  // Gutter marker + squiggle + line highlight via the lint extension.
  const cmDiags = diags
    .filter((d) => d.line)
    .map((d) => {
      const line = view.state.doc.line(Math.min(d.line, view.state.doc.lines));
      const from = d.col ? Math.min(line.from + d.col - 1, line.to) : line.from;
      return {
        from,
        to: line.to,
        severity: d.severity === "warning" ? "warning" : "error",
        message: d.message,
      };
    });
  view.dispatch(setDiagnostics(view.state, cmDiags));
}

// ---------- Download ----------

document.getElementById("download").onclick = async () => {
  setStatus("preparing download…");
  try {
    const res = await fetch(`/${boot.slug}/compile?download=1`, {
      method: "POST",
      headers: { "Content-Type": "text/plain; charset=utf-8" },
      body: view.state.doc.toString(),
    });
    if (!res.ok) {
      setStatus("download failed: source has errors");
      return;
    }
    const blob = await res.blob();
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    const dispo = res.headers.get("content-disposition") || "";
    a.download = (dispo.match(/filename="([^"]+)"/) || [])[1] || `${boot.slug}.pdf`;
    a.click();
    URL.revokeObjectURL(a.href);
    setStatus("");
  } catch {
    setStatus("download failed");
  }
};

// ---------- Split divider ----------

const divider = document.getElementById("divider");
divider.addEventListener("pointerdown", (down) => {
  down.preventDefault();
  divider.setPointerCapture(down.pointerId);
  const onMove = (move) => {
    const rect = splitEl.getBoundingClientRect();
    const frac = Math.min(0.8, Math.max(0.2, (move.clientX - rect.left) / rect.width));
    splitEl.style.gridTemplateColumns = `${frac}fr 6px ${1 - frac}fr`;
  };
  const onUp = () => {
    divider.removeEventListener("pointermove", onMove);
    divider.removeEventListener("pointerup", onUp);
  };
  divider.addEventListener("pointermove", onMove);
  divider.addEventListener("pointerup", onUp);
});

// ---------- Responsive code/preview toggle ----------

const toggleBtn = document.getElementById("toggle");
const narrow = window.matchMedia("(max-width: 700px)");

function applyNarrow() {
  toggleBtn.hidden = !narrow.matches;
  document.body.classList.toggle("narrow", narrow.matches);
  if (!narrow.matches) document.body.classList.remove("show-preview");
}
narrow.addEventListener("change", applyNarrow);
applyNarrow();

toggleBtn.onclick = () => {
  const showPreview = document.body.classList.toggle("show-preview");
  toggleBtn.textContent = showPreview ? "Code" : "Preview";
};

// ---------- Boot ----------

compile();
