# typst-letter — Build Specification

Hand this to an implementing agent. It is complete and self-contained.

## 1. Overview
A self-hosted web service for writing letters in Typst. The browser shows an index of templates. Opening a template gives a split-view editor: CodeMirror on the left with the template's Typst source preloaded, a live PDF preview on the right. Editing triggers a debounced recompile. Typst is compiled in-process via `typst-as-lib` (no CLI subprocess). Signature image and shared styles live server-side. Ships as a single Rust binary plus a `templates/` directory.

## 2. Tech stack
- Language: Rust (stable).
- Web framework: `axum` + `tokio`.
- Typst: `typst-as-lib` (compile), `typst-pdf` (PDF export), `typst-kit` (fonts + file resolver helpers).
- Frontend: vanilla JS + CodeMirror 6, bundled and served locally (no CDN, must work offline).
- Config: `toml` via `serde`.
- Packaging: `buildRustPackage` + a NixOS module.

Pin the `typst-as-lib` version and the matching `typst-pdf`/`typst-kit` versions in `Cargo.toml`; they must agree on the underlying Typst compiler version. Reference: https://crates.io/crates/typst-as-lib and https://docs.rs/typst-kit.

## 3. Directory layout
```
typst-letter/
  Cargo.toml
  src/
    main.rs           # config load, router, serve
    routes.rs         # handlers: index, editor, compile, static
    compiler.rs       # CompilerPool, engine reuse, diagnostics mapping
    resolver.rs       # confined file/source resolver (security boundary)
    config.rs         # Config struct + defaults
    templates.rs      # discover + read templates from disk
  static/
    editor.js         # CodeMirror bundle + app logic
    editor.css
  templates/          # user data, not part of the binary
    shared/
      letter.typ
      signature.png
    business.typ
    personal.typ
  config.toml
  nix/module.nix
  flake.nix
```

## 4. Template model
- A template is a file `templates/{slug}.typ`. The slug is the filename stem.
- `templates/shared/` holds `letter.typ` (imported by templates) and `signature.png`. `shared` is not itself a template and must not be routable.
- Slug rule: must match `^[a-z0-9-]+$` and resolve to an existing `templates/{slug}.typ`. Reject `shared`, empty, or anything containing `/`, `.`, or `..`.
- The on-disk `.typ` is the pristine starting point. v1 never writes to it. Browser edits are per-session only.

## 5. HTTP API
| Method | Path | Request | Response |
|---|---|---|---|
| GET | `/` | — | HTML index: one link per template, showing slug and the template's title (see 5.1) |
| GET | `/{slug}` | — | HTML editor page; embeds the template source as the editor's initial content |
| POST | `/{slug}/compile` | body: raw Typst source (`text/plain`, UTF-8) | `200` `application/pdf` on success; `422` `application/json` on compile error |
| GET | `/static/*` | — | Static editor assets |
| GET | `/healthz` | — | `200 "ok"` |

Add `?download=1` to a successful compile response: same bytes but `Content-Disposition: attachment`. Default is `inline`. Since compile is POST, the download variant is a query param on the POST URL.

**5.1 Title extraction:** the index shows, per template, the first non-empty line of the file with a leading `//` comment stripped, falling back to the slug. Purely cosmetic.

**5.2 Compile success response:**
- `Content-Type: application/pdf`
- `Content-Disposition: inline; filename="{slug}-{YYYY-MM-DD}.pdf"` (attachment if `download=1`)
- Body: PDF bytes.

**5.3 Compile error response (`422`):** JSON array of diagnostics:
```json
[{ "severity": "error", "message": "unknown variable: reciptient", "line": 12, "col": 3 }]
```
`line`/`col` are 1-based, mapped from Typst's spans to the submitted source. If a span can't be resolved to a line, omit `line`/`col` but keep `message`. Warnings may be included with `"severity":"warning"`; the client shows them but still renders the PDF if one was produced.

## 6. Render pipeline (`compiler.rs`)
1. Reject bodies larger than `max_source` (default 256 KiB) with `413`.
2. Acquire a compile slot from the `CompilerPool` (bounded, see 6.2). If none available within a short wait, return `429`.
3. Compile the submitted source as the main file, using a reused engine (6.1) with the confined resolver (7) and configured fonts (8).
4. On success, export with `typst-pdf` to bytes, return per 5.2.
5. On failure, map diagnostics per 5.3, return `422`.
6. Enforce a per-compile wall-clock timeout (`compile_timeout`, default 10s) and catch panics; either yields a `500` with a generic message (do not leak internals to the client, log full detail server-side).

**6.1 Engine reuse:** build the `typst-as-lib` engine once with fonts + resolver, and swap only the main source per compile. Do not reconstruct fonts/resolver per request. `typst-as-lib` explicitly supports an editable main file between compile calls; rely on Typst's incremental compilation (`comemo`) for cheap repeat compiles of mostly-unchanged input. Keep one engine per compile worker.

**6.2 CompilerPool:** a fixed pool of `max_compiles_in_flight` workers (default 4), each owning its own engine (engines are not `Sync`). A bounded MPSC queue feeds them. Requests beyond queue capacity get `429`. This caps CPU across many tabs/clients.

## 7. Security boundary — confined resolver (`resolver.rs`)
The browser submits arbitrary Typst. The resolver is the only thing preventing filesystem access. Requirements:
- All `import`, `read`, `image`, and package path resolution must resolve **only within `templates/`** (canonicalize, then verify the result is still under the canonical templates root).
- Reject absolute paths and any path escaping the root (`..`), at the resolver level, not just the router.
- No writes are ever performed by the resolver.
- Typst has no network/shell access by itself; with reads confined, worst case for a hostile paste is reading the templates tree. Acceptable for a personal/VPN instance; document this.

Additionally the router-level slug guard (4) must run before anything touches disk.

## 8. Fonts & packages (`typst-kit`)
- Enable `typst-kit` font features: embed a small default serif+sans, and scan `font_paths` from config plus system fonts, so `set text(font: ...)` works.
- Universe packages (`@preview/...`): disabled by default (offline-first). Provide a config flag `allow_universe = false`. When false, package imports must fail with a clear diagnostic. When true, use `typst-kit`'s Universe package loader with on-disk caching. For letters, prefer vendoring shared code into `templates/shared/` and importing by path.

## 9. Frontend (`static/`)
**Layout:** CSS grid, two columns with a draggable divider. Left: CodeMirror 6. Right: PDF preview via `<embed type="application/pdf">` (acceptable v1) — or PDF.js if the agent wants scroll/zoom control; `<embed>` is the default to keep it simple. Below the editor: a thin diagnostics strip.

**Responsive:** below ~700px width, stack vertically and show a Code/Preview toggle instead of side-by-side.

**Editor:** CodeMirror 6 with a Typst mode. Prefer an existing `codemirror-lang-typst` grammar; if unavailable or unstable, implement a minimal `StreamLanguage` highlighting headings (`=`…), `#` code/function calls, `*strong*`, `_emph_`, strings, and comments. Preload with the template source from the page. Persist unsaved edits to `localStorage` keyed by slug; restore on load.

**Live recompile:**
- Debounce `debounce_ms` (default 500ms, value injected from server config into the page) after the last keystroke, then POST the current source to `/{slug}/compile`.
- Use `AbortController`; cancel any in-flight compile when a newer edit fires. Never let an older response overwrite a newer one (track a request sequence number).
- While compiling, keep the last good PDF visible with a subtle "compiling…" indicator.
- On `200`, replace the preview (revoke the previous object URL to avoid leaks).
- On `422`, keep the last good PDF, render diagnostics in the strip, and place a gutter marker + line highlight in CodeMirror at each diagnostic's `line`.
- On `429`, back off briefly and retry once.
- A "Download PDF" button re-POSTs with `?download=1` (or reuses the last successful blob).

**Offline:** all JS/CSS served from `/static`; no external network calls from the page.

## 10. Configuration (`config.toml`)
```toml
listen                 = "127.0.0.1:8080"
templates_dir          = "./templates"
font_paths             = ["./.fonts"]
max_source             = "256KiB"
compile_timeout        = "10s"
debounce_ms            = 500
max_compiles_in_flight = 4
allow_universe         = false
base_title             = "Letters"
# auth = { user = "alice", pass = "…" }   # optional HTTP Basic auth
```
- Human-readable sizes/durations parsed into bytes/`Duration`.
- If `auth` is set, gate all routes except `/healthz` behind HTTP Basic. Default: no auth (intended to sit behind a reverse proxy / VPN).
- Fail fast with a clear message if `templates_dir` is missing or unreadable.

## 11. Logging & errors
- Structured logging (`tracing`): log every compile with slug, source size, duration, and outcome (ok / error / timeout / panic).
- Client-facing errors are generic; full detail stays in logs.
- No telemetry, no external calls.

## 12. NixOS module (`nix/module.nix`)
Expose `services.typst-letter`:
```
enable, listen, templatesDir, fontPaths, package
```
- systemd service running the binary as a dedicated unprivileged user.
- Hardening: `templatesDir` mounted read-only to the service; `ProtectSystem=strict`, `ProtectHome=true`, `PrivateTmp=true`, `NoNewPrivileges=true`, no `ReadWritePaths` (the service performs no runtime writes).
- Provide `flake.nix` with `buildRustPackage` for the binary and the module as a `nixosModules.default` output.

Example usage the module should support:
```nix
services.typst-letter = {
  enable = true;
  listen = "127.0.0.1:8080";
  templatesDir = "/var/lib/typst-letter/templates";
  fontPaths = [ "/var/lib/typst-letter/.fonts" ];
};
```
TLS/exposure is out of scope for the module; document putting Caddy or nginx in front.

## 13. Seed content (ship in `templates/`)
**`templates/shared/letter.typ`** — the reusable letter function:
```typst
#let letter(
  name: "John Doe",
  address: none,
  contact: (:),
  recipient: [],
  subject: none,
  date: datetime.today().display("[month repr:long] [day], [year]"),
  closing: "Sincerely,",
  signature: none,
  body,
) = {
  set page(paper: "us-letter", margin: (x: 1in, y: 1in))
  set text(font: "Libertinus Serif", size: 11pt, lang: "en")
  set par(justify: true, leading: 0.6em)

  align(right, text(size: 9pt)[
    *#name* \
    #address
    #for (_, v) in contact [ \ #v]
  ])
  v(1.5em)
  recipient
  v(1.5em)
  align(right, date)
  v(1em)
  if subject != none { strong(subject); v(1em) }
  body
  v(1.5em)
  closing
  if signature != none { v(0.2em); image(signature, height: 1.6cm) }
  v(0.2em)
  name
}
```

**`templates/business.typ`** — an editable starting point:
```typst
#import "shared/letter.typ": letter

#show: letter.with(
  name: "Jane Roe",
  address: [123 Main St \ Portland, OR 97201],
  contact: (email: "jane@roe.com", phone: "+1 555 0100"),
  recipient: [Acme Corp. \ Sesame Street 23 \ 12345 Gotham City],
  subject: [Re: Your inquiry],
  signature: "shared/signature.png",
)

Dear Acme team,

Lorem ipsum dolor sit amet, consectetuer adipiscing elit.

Sincerely,
```

Ship a placeholder `shared/signature.png` (the user replaces it) and a second `templates/personal.typ` with a lighter, non-business variant so the index shows more than one entry.

## 14. Acceptance criteria
1. `nix build` (or `cargo build --release`) produces a single runnable binary.
2. Running it with the seed `templates/` serves `/`, listing `business` and `personal` with titles.
3. `GET /business` returns the split-view editor with the source preloaded and syntax highlighted.
4. Typing in the editor, then pausing, recompiles within the debounce window and updates the right-hand PDF without a full page reload.
5. Introducing a Typst error keeps the last good PDF, shows the diagnostic in the strip, and marks the offending line in the gutter.
6. The seed `business.typ` compiles to a US-letter PDF with the signature image rendered above the sender name.
7. An edit that tries to `read`/`image`/`import` a path outside `templates/` (e.g. `image("/etc/passwd")` or `import "../../secret"`) fails with a resolver error and never reads the file.
8. A slug outside `^[a-z0-9-]+$`, or `shared`, returns `404`/`400` and never touches disk.
9. Rapid edits never let an older compile result overwrite a newer one (verified by sequence/abort handling).
10. With `auth` set, all routes except `/healthz` require valid Basic credentials.
11. The page makes no external network requests (verifiable in devtools with the network offline).
12. The systemd unit runs as a non-root user with `templatesDir` read-only.

## 15. Explicit non-goals (v1)
Saving edits back to disk, draft sync across devices, accounts/multi-user, WYSIWYG, per-keystroke (non-debounced) compile, template creation via UI. Templates are authored by editing `.typ` files on disk.
