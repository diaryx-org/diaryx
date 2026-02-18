{
  description = "Diaryx - Command-line interface and development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          inherit (pkgs) lib;

          rustToolchain = pkgs.rust-bin.stable.latest.default;

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              (lib.fileset.fileFilter (file: file.hasExt "rs") ./.)
              (lib.fileset.fileFilter (file: file.hasExt "toml") ./.)
              (lib.fileset.fileFilter (file: file.name == "Cargo.lock") ./.)
              (lib.fileset.fileFilter (file: file.hasExt "md") ./.)
              (lib.fileset.fileFilter (file: file.hasExt "json") ./.)
              (lib.fileset.fileFilter (file: file.hasExt "png") ./.)
            ];
          };

          diaryx-cli = (pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          }).buildRustPackage {
            pname = "diaryx";
            version = "1.0.1";
            inherit src;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "diaryx" ];
            cargoTestFlags = [ "-p" "diaryx" ];
            doCheck = false;

            # Fixed: Use the modern SDK 15 attribute directly
            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk_15
            ];

            nativeBuildInputs = [ pkgs.pkg-config ];
          };
        in
        {
          default = diaryx-cli;
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/diaryx";
        };
      });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            targets = [
              "aarch64-apple-darwin"
              "x86_64-unknown-linux-gnu"
              "wasm32-unknown-unknown"
            ];
            extensions = [ "rust-src" "rust-analyzer" ];
          };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustToolchain
              zig
              cargo-zigbuild
              cargo-tauri
              cargo-binstall
              bun
              pkg-config
              prek
              llvmPackages.lld
              openssl.dev
            ];

            # Fixed: Using apple-sdk_15 directly avoids the legacy 11.0 stub error
            buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk_15
            ] ++ [ pkgs.openssl ];

            shellHook = ''
              export ZIG_GLOBAL_CACHE_DIR="$PWD/.zig-cache"

              # Force clean the environment of legacy SDK markers
              unset DEVELOPER_DIR
              unset SDKROOT

              echo "Welcome to the Diaryx development environment!"
              echo "Targets enabled: x86_64-linux, aarch64-darwin, wasm32"
              echo "Rust: $(rustc --version)"
            '';
          };
        });
    };
}
