{
  description = "typst-letter — self-hosted Typst letter editor";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        typst-letter = pkgs.rustPlatform.buildRustPackage {
          pname = "typst-letter";
          version = "0.1.0";
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let rel = pkgs.lib.removePrefix (toString ./. + "/") (toString path);
              in !(pkgs.lib.hasPrefix "frontend/node_modules" rel
                || pkgs.lib.hasPrefix "docs" rel
                || pkgs.lib.hasPrefix "target" rel);
          };
          cargoLock.lockFile = ./Cargo.lock;
          # network tests don't run in the sandbox; unit tests are hermetic
          doCheck = true;
          meta = {
            description = "Self-hosted web service for writing letters in Typst";
            license = { spdxId = "PolyForm-Noncommercial-1.0.0"; free = false; };
            mainProgram = "typst-letter";
          };
        };
      in
      {
        packages.default = typst-letter;
        devShells.default = pkgs.mkShell {
          inputsFrom = [ typst-letter ];
          packages = [ pkgs.nodejs pkgs.just ];
        };
      })
    // {
      nixosModules.default = import ./nix/module.nix self;
    };
}
