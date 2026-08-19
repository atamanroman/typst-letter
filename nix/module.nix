# NixOS module for typst-letter.
#
# TLS/exposure is out of scope: run behind a reverse proxy (Caddy, nginx)
# or keep it on a VPN interface.
flake:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.typst-letter;
  settings = {
    listen = cfg.listen;
    templates_dir = cfg.templatesDir;
    font_paths = cfg.fontPaths;
  };
  configFile = (pkgs.formats.toml { }).generate "typst-letter.toml" settings;
in
{
  options.services.typst-letter = {
    enable = lib.mkEnableOption "typst-letter, a web editor for Typst letters";

    listen = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:8080";
      description = "Address and port to listen on.";
    };

    templatesDir = lib.mkOption {
      type = lib.types.path;
      example = "/var/lib/typst-letter/templates";
      description = "Directory holding the letter templates (mounted read-only).";
    };

    fontPaths = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ ];
      description = "Extra font directories to scan.";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = "typst-letter from this flake";
      description = "The typst-letter package to run.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.typst-letter = {
      description = "typst-letter web service";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} ${configFile}";
        DynamicUser = true;
        Restart = "on-failure";

        # The service performs no runtime writes.
        BindReadOnlyPaths = [ cfg.templatesDir ] ++ cfg.fontPaths;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        RestrictNamespaces = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        CapabilityBoundingSet = "";
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
        SystemCallFilter = [ "@system-service" "~@privileged" ];
      };
    };
  };
}
