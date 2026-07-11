# typst-letter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Self-hosted web service for writing letters in Typst: template index, split-view CodeMirror editor with live-recompiled PDF preview, compiled in-process.

**Architecture:** One axum binary. A `CompilerPool` of N OS threads, each owning a reused `typst-as-lib` engine fed by a bounded mpsc channel; handlers await a oneshot reply with a wall-clock timeout. A confined `FileResolver` is the only filesystem access path for Typst code. Frontend is a prebuilt esbuild bundle (CodeMirror 6 + `codemirror-lang-typst`) served from `/static`, checked into the repo so `cargo build` alone suffices.

**Tech Stack:** Rust stable, axum 0.8, tokio, typst-as-lib 0.16 (typst 0.15), typst-pdf 0.15, serde/toml, tracing; vanilla JS + CodeMirror 6 bundled with esbuild; Nix flake + NixOS module; justfile for commands.

## Global Constraints

- Pinned versions: `typst-as-lib = "0.16"`, `typst = "0.15"`, `typst-pdf = "0.15"` (must agree on compiler version).
- typst-as-lib features: `typst-kit-fonts`, `typst-kit-embed-fonts`, `packages`, `ureq`.
- Slug rule: `^[a-z0-9-]+$`, not `shared`, must resolve to existing `templates/{slug}.typ`.
- Defaults: `listen=127.0.0.1:8080`, `max_source=256KiB`, `compile_timeout=10s`, `debounce_ms=500`, `max_compiles_in_flight=4`, `allow_universe=false`, `base_title="Letters"`.
- Client-facing errors generic; detail only in `tracing` logs. No telemetry, no external calls, page works offline.
- v1 never writes template files; browser edits are per-session (localStorage only).

---

### Task 1: Scaffold + config.rs

**Files:** Create `Cargo.toml`, `src/main.rs` (stub), `src/config.rs`, `config.toml`, `justfile`, `.gitignore`.

**Interfaces produces:** `Config { listen: SocketAddr, templates_dir: PathBuf, font_paths: Vec<PathBuf>, max_source: usize, compile_timeout: Duration, debounce_ms: u64, max_compiles_in_flight: usize, allow_universe: bool, base_title: String, auth: Option<BasicAuth{user,pass}> }`, `Config::load(path) -> anyhow::Result<Config>`. Human-readable size (`KiB/MiB/B`) and duration (`s/ms/m`) parsers with unit tests. Fail fast if `templates_dir` missing/unreadable.

Steps: write parser tests (size: `"256KiB"`→262144, `"1MiB"`, bare bytes; duration: `"10s"`, `"500ms"`; defaults when fields omitted; auth optional) → fail → implement → pass → commit.

### Task 2: templates.rs — discovery, slug guard, titles

**Interfaces produces:** `valid_slug(&str) -> bool`; `list_templates(dir) -> Vec<TemplateMeta{slug,title}>` (sorted, skips `shared/`); `read_template(dir, slug) -> Option<String>`; `extract_title(src, slug) -> String` (first non-empty line, leading `//` stripped, fallback slug).

Tests: slug rejects `shared`, ``, `a/b`, `a.b`, `..`, uppercase, unicode; accepts `business`, `my-letter2`. Title extraction: comment line, plain line, empty file → slug. Discovery via tempdir fixture. TDD cycle, commit.

### Task 3: resolver.rs — confined resolver (security boundary)

**Interfaces produces:**
- `ConfinedResolver::new(root: PathBuf, allow_universe: bool)` implementing `typst_as_lib::file_resolver::FileResolver` (`resolve_binary`/`resolve_source`).
- `MainSlot` resolver: fixed FileId `/__main__.typ` (`RootedPath::new(VirtualRoot::Project, VirtualPath::new(..)).intern()`), `set_source(&self, text: &str)` via `RwLock<Source>` + `Source::replace` (incremental reparse), `MainSlot::file_id()`.

Resolution: FileId with `VirtualRoot::Package(_)` → if `allow_universe` false, `FileError::Other("universe packages disabled (allow_universe = false)")`; project paths joined to root, **canonicalized, verified under canonical root**; absolute/`..`-escaping paths rejected at this level. No writes ever.

Tests: resolves `shared/letter.typ` inside fixture root; rejects `../outside.typ`, `/etc/passwd` (both must not read the file — assert error, use file outside root that exists); package id rejected with clear message when disabled. TDD cycle, commit.

### Task 4: compiler.rs — CompilerPool, diagnostics, timeout

**Interfaces produces:**
- `CompilerPool::new(cfg: &Config) -> CompilerPool` — spawns `max_compiles_in_flight` std threads; each builds one `TypstEngine` (fonts via `search_fonts_with(TypstKitFontOptions::new().include_dirs(font_paths))`, `add_file_resolver(MainSlot)`, `add_file_resolver(ConfinedResolver)`, `with_package_file_resolver()` iff `allow_universe`). Bounded `tokio::sync::mpsc` queue (capacity = 2×workers).
- `pool.compile(source: String) -> CompileOutcome` (async): `Queued` full → `Busy` (429); reply awaited under `tokio::time::timeout(compile_timeout)` → `TimedOut`; worker `catch_unwind` → `Panicked`.
- `CompileOutcome::Ok { pdf: Vec<u8>, warnings: Vec<Diag> } | Err { diags: Vec<Diag> } | Busy | TimedOut | Panicked`.
- `Diag { severity: "error"|"warning", message: String, line: Option<u32>, col: Option<u32> }` (serde Serialize) — spans mapped via main `Source` (`range`, `byte_to_line`/`byte_to_column`, +1); non-main spans → message only.

Worker loop: `slot.set_source(&src)`; `engine.compile::<PagedDocument>(MainSlot::file_id())`; on ok `typst_pdf::pdf(&doc, &PdfOptions::default())`. Log slug/size/duration/outcome via `tracing`.

Tests (integration, seed fixture): trivial source → PDF magic `%PDF`; `#foo` undefined → error diag with line/col 1-based; `image("/etc/passwd")` → 422-class error not panic. Commit.

### Task 5: routes.rs + main.rs — HTTP surface

Routes per spec §5: `GET /` index HTML; `GET /{slug}` editor HTML (source JSON-embedded, debounce_ms injected); `POST /{slug}/compile` (?download=1 → attachment; body limit `max_source` → 413; Content-Disposition `{slug}-{YYYY-MM-DD}.pdf`); `GET /static/*`; `GET /healthz`. Slug guard before any disk access (400/404). Optional Basic auth middleware exempting `/healthz`. HTML built with `format!` templates (no engine dep), escape source via JSON embed `<script type="application/json">`.

Tests: `tower::ServiceExt::oneshot` — healthz 200; bad slug 400/404 without disk touch; index lists seeds; compile happy path returns `application/pdf` inline; download=1 → attachment; oversized body 413; auth on/off. Commit.

### Task 6: Seed templates

`templates/shared/letter.typ`, `templates/business.typ` (verbatim from spec §13), `templates/personal.typ` (lighter variant), placeholder `templates/shared/signature.png` (generated scribble PNG). Verify business.typ compiles via pool test. Commit.

### Task 7: Frontend

**Files:** `frontend/` (npm project: `package.json`, `src/editor.js`), built to `static/editor.js` + `static/editor.css` via esbuild (`just frontend`), bundle committed.

Editor: CodeMirror 6 + `codemirror-lang-typst` (fallback StreamLanguage if broken at runtime check during build). Grid split w/ draggable divider; <700px stacked + Code/Preview toggle; diagnostics strip; gutter markers + line highlight per diag; debounce (server-injected); AbortController + seq number (older never overwrites newer); keep last good PDF + "compiling…" indicator; revoke old object URLs; 429 → backoff retry once; Download button; localStorage persistence keyed `typst-letter:{slug}`; `<embed type="application/pdf">`. No external requests. Commit bundle + sources.

### Task 8: Nix packaging

`flake.nix` (`buildRustPackage`, `nixosModules.default`), `nix/module.nix` (`services.typst-letter`: enable/listen/templatesDir/fontPaths/package; DynamicUser, `BindReadOnlyPaths`/read-only templatesDir, `ProtectSystem=strict`, `ProtectHome=true`, `PrivateTmp=true`, `NoNewPrivileges=true`, no ReadWritePaths). README documents reverse-proxy for TLS + hostile-paste threat model (§7). `nix build` must pass. Commit.

### Task 9: Acceptance pass

Run binary with seed templates; curl checks for criteria §14 (1–12 where automatable); fix gaps; final commit.
