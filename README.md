# typst-letter

Self-hosted web service for writing letters in [Typst](https://typst.app/).
The index lists your templates; opening one gives a split-view editor
(CodeMirror left, live PDF preview right) with debounced recompiles.
Typst is compiled in-process — no CLI subprocess. Ships as a single Rust
binary plus a `templates/` directory.

## Quick start

```sh
just run          # cargo run with ./config.toml
just test         # run the test suite
just build        # release binary
just frontend     # rebuild static/editor.js from frontend/ (bundle is committed)
```

Open http://127.0.0.1:8080, pick `business` or `personal`, type, pause,
watch the PDF update.

## Templates

A template is a file `templates/{slug}.typ`; the slug (`^[a-z0-9-]+$`) is
the filename stem. `templates/shared/` holds `letter.typ` (the reusable
letter function) and `signature.png` (replace the placeholder with your
own scan) and is not itself routable. The on-disk `.typ` files are pristine
starting points: the server never writes to them, browser edits live in
`sessionStorage` only (drafts survive reloads, die with the tab). Note
that Typst resolves relative paths against the
file using them, so pass project-rooted paths like `/shared/signature.png`
as arguments.

## Configuration

See `config.toml`. Sizes ("256KiB") and durations ("10s") are
human-readable. Set `auth = { user = "…", pass = "…" }` to gate everything
except `/healthz` behind HTTP Basic auth; by default there is none, on the
assumption the service sits behind a reverse proxy or VPN.

Universe packages (`@preview/...`) are disabled by default (offline-first,
`allow_universe = false`); vendor shared code into `templates/shared/`
instead. When enabled, packages are fetched and cached on disk.

## Security model

The browser submits arbitrary Typst source. Compilation is confined by a
resolver that only reads inside `templates/` (path canonicalization plus
root prefix check, symlink escapes included). Typst itself has no network
or shell access, so the worst case for a hostile paste is *reading files
under the templates tree*. That is acceptable for a personal or VPN-bound
instance — do not expose the service unauthenticated to the open internet.

A fixed pool of compile workers (`max_compiles_in_flight`) with a bounded
queue caps CPU; excess requests get 429. Oversized sources get 413, and a
wall-clock timeout aborts requests whose compilation hangs.

## NixOS

```nix
{
  inputs.typst-letter.url = "github:you/typst-letter";

  # in your system config:
  imports = [ typst-letter.nixosModules.default ];
  services.typst-letter = {
    enable = true;
    listen = "127.0.0.1:8080";
    templatesDir = "/var/lib/typst-letter/templates";
    fontPaths = [ "/var/lib/typst-letter/.fonts" ];
  };
}
```

The unit runs as a dynamic unprivileged user with `templatesDir` bound
read-only, `ProtectSystem=strict`, `ProtectHome=true`, `PrivateTmp=true`,
`NoNewPrivileges=true`, and no writable paths. TLS is out of scope — put
Caddy or nginx in front.

## Development

`frontend/` holds the esbuild project for the editor bundle; the built
`static/editor.js` is committed so plain `cargo build` (and `nix build`)
need no Node. The bundle embeds CodeMirror 6 with a minimal Typst
highlighter; everything is served locally, the page makes no external
requests.

## License

Copyright © 2026 Roman Ataman. The source is available under the
[PolyForm Noncommercial License 1.0.0](LICENSE), which permits noncommercial
use. Commercial use requires separate permission.
