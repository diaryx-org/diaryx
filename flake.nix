{
  description = "Diaryx - Command-line interface and development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          localSystem = system;
          overlays = [ (import rust-overlay) ];
        };

        inherit (pkgs) lib;

        rustToolchain = pkgs.rust-bin.stable."1.91.0".default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            # Include all standard Rust/Cargo files (the default)
            (craneLib.fileset.commonCargoSources ./.)
            # Explicitly include all Markdown files for documentation
            (lib.fileset.fileFilter (file: file.hasExt "md") ./.)
          ];
        };

        commonArgs = {
          inherit src;
          pname = "diaryx";
          version = "0.11.0";
          strictDeps = true;

          buildInputs = [
            pkgs.stdenv.cc.cc.lib
          ] ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.apple-sdk_15
          ];

          nativeBuildInputs = [
            pkgs.pkg-config
          ] ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.apple-sdk_15
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        diaryx-cli = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "diaryx";
          cargoExtraArgs = "-p diaryx";
        });

      in
      {
        packages.default = diaryx-cli;

        apps.default = flake-utils.lib.mkApp {
          drv = diaryx-cli;
        };

        devShells.default = craneLib.devShell {
          inputsFrom = [ diaryx-cli ];

          packages = with pkgs; [
            rustToolchain
            cargo-release
            cargo-binstall
            wasm-pack
            bun
          ];

          shellHook = ''
            echo "Welcome to the Diaryx development environment!"
            echo "Rust: $(rustc --version)"
            echo "Bun:  $(bun --version)"
          '';
        };
      });
}
