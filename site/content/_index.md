+++
title = "Home"
template = "index.html"
+++

# private letters, live preview

`letters` is a self-hosted web editor for writing letters in [Typst](https://typst.app/).
Pick a template, edit it beside a live PDF preview, then download the finished letter.

Drafts stay in the browser tab and disappear with it.
The server compiles Typst in-process, never writes edited source to disk, and makes no external requests by default.

## What it does

- Lists reusable personal and business letter templates.
- Recompiles after each pause and keeps the last good preview visible.
- Shows Typst diagnostics beside the source.
- Stores temporary edits in `sessionStorage`, not on the server.
- Ships as one Rust binary with a templates directory.
- Supports HTTP Basic authentication or deployment behind a private reverse proxy.

## Install

Build and run it from source:

```console
git clone https://github.com/atamanroman/typst-letter.git
cd typst-letter
just build
./target/release/typst-letter
```

The default configuration listens on `127.0.0.1:8080` and reads templates from `./templates`.
Put a TLS reverse proxy in front before exposing it beyond a trusted network.

### NixOS

Add the flake and enable its module:

```nix
{
  inputs.typst-letter.url = "github:atamanroman/typst-letter";

  imports = [ inputs.typst-letter.nixosModules.default ];

  services.typst-letter = {
    enable = true;
    listen = "127.0.0.1:8080";
    templatesDir = "/var/lib/typst-letter/templates";
  };
}
```

The module runs the service as a dynamic unprivileged user with the templates directory mounted read-only and a hardened systemd unit.

Read the [full documentation and source](https://github.com/atamanroman/typst-letter).
