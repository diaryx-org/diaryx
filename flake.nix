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
              (lib.fileset.fileFilter (file: file.hasExt "der") ./.)
            ];
          };

          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };

          wasmRustToolchain = pkgs.rust-bin.stable.latest.default.override {
            targets = [ "wasm32-unknown-unknown" ];
          };

          wasmRustPlatform = pkgs.makeRustPlatform {
            cargo = wasmRustToolchain;
            rustc = wasmRustToolchain;
          };

          diaryx-cli = rustPlatform.buildRustPackage {
            pname = "diaryx";
            version = "1.4.5";
            inherit src;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "diaryx" ];
            cargoTestFlags = [ "-p" "diaryx" ];
            doCheck = false;

            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk_15
            ];

            nativeBuildInputs = [ pkgs.pkg-config ];
          };

          diaryx-sync-server = rustPlatform.buildRustPackage {
            pname = "diaryx-sync-server";
            version = "1.4.5";
            inherit src;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "diaryx_sync_server" ];
            doCheck = false; # tests need network/DB

            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk_15
            ];

            # perl needed for vendored openssl build
            nativeBuildInputs = [ pkgs.pkg-config pkgs.perl ];
          };

          ts-bindings = rustPlatform.buildRustPackage {
            pname = "ts-bindings";
            version = "1.4.5";
            inherit src;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "diaryx_core" ];
            cargoTestFlags = [ "-p" "diaryx_core" ];
            doCheck = true; # ts-rs generates bindings during cargo test

            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk_15
            ];

            nativeBuildInputs = [ pkgs.pkg-config ];

            postCheck = ''
              # ts-rs writes bindings relative to the crate directory
              if [ -d crates/diaryx_core/bindings ]; then
                mkdir -p $out
                cp -r crates/diaryx_core/bindings/* $out/
              fi
            '';

            # Skip the normal install phase (we only want the test output)
            installPhase = ''
              # Bindings already copied in postCheck
              if [ ! -d "$out" ] || [ -z "$(ls -A "$out" 2>/dev/null)" ]; then
                echo "Warning: no bindings were generated"
                mkdir -p $out
              fi
            '';
          };

          # wasm-bindgen-cli must match the wasm-bindgen crate version in Cargo.lock.
          # If wasm-bindgen is upgraded, update this version and fix hashes by running:
          #   nix build .#wasm-package
          # and replacing the hashes from the error output.
          wasm-bindgen-cli = pkgs.buildWasmBindgenCli rec {
            src = pkgs.fetchCrate {
              pname = "wasm-bindgen-cli";
              version = "0.2.118";
              hash = "sha256-ve783oYH0TGv8Z8lIPdGjItzeLDQLOT5uv/jbFOlZpI=";
            };
            cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
              inherit src;
              inherit (src) pname version;
              hash = "sha256-EYDfuBlH3zmTxACBL+sjicRna84CvoesKSQVcYiG9P0=";
            };
          };

          wasm-package = wasmRustPlatform.buildRustPackage {
            pname = "wasm-package";
            version = "1.4.5";
            inherit src;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "diaryx_wasm" "--target" "wasm32-unknown-unknown" ];
            doCheck = false;

            nativeBuildInputs = [
              wasm-bindgen-cli
              pkgs.binaryen
              pkgs.pkg-config
            ];

            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk_15
            ];

            # Override install to run wasm-bindgen + wasm-opt instead of cargo install
            installPhase = ''
              mkdir -p $out
              wasm-bindgen --target web --out-dir $out \
                target/wasm32-unknown-unknown/release/diaryx_wasm.wasm
              wasm-opt -Oz -o $out/diaryx_wasm_bg.wasm $out/diaryx_wasm_bg.wasm

              # Generate package.json for npm publishing compatibility
              cat > $out/package.json <<'EOF'
            {
              "name": "diaryx_wasm",
              "type": "module",
              "version": "1.4.0",
              "main": "diaryx_wasm.js",
              "types": "diaryx_wasm.d.ts",
              "files": ["diaryx_wasm_bg.wasm", "diaryx_wasm.js", "diaryx_wasm.d.ts", "diaryx_wasm_bg.wasm.d.ts"]
            }
            EOF
            '';
          };
        in
        {
          default = diaryx-cli;
          inherit diaryx-sync-server ts-bindings wasm-package;
          "wasm-bindgen-cli" = wasm-bindgen-cli;
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
              "aarch64-apple-ios"
              "aarch64-apple-ios-sim"
            ];
            extensions = [ "rust-src" "rust-analyzer" "llvm-tools-preview" ];
          };
        in
        {
          default = pkgs.mkShellNoCC {
            packages = with pkgs; [
              rustToolchain
              cargo-bloat
              twiggy
              cargo-llvm-cov
              self.packages.${system}."wasm-bindgen-cli"
              zig
              cargo-zigbuild
              cargo-tauri
              cargo-binstall
              bun
              pkg-config
              prek
              llvmPackages.lld
              openssl.dev
              binaryen
            ];

            # Add libiconv for macOS host build scripts
            buildInputs = [ pkgs.openssl pkgs.libiconv ];

            shellHook = ''
              repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

              export ZIG_GLOBAL_CACHE_DIR="$PWD/.zig-cache"
              export NODE_PATH="$repo_root/crates/diaryx_cloudflare/test-support/node_modules''${NODE_PATH:+:$NODE_PATH}"

              # Force clean the environment of legacy SDK markers
              unset DEVELOPER_DIR
              unset SDKROOT
              unset MACOSX_DEPLOYMENT_TARGET

              # Bypass Nix's cc-wrapper for iOS targets because it injects macOS min version flags
              export CC=/usr/bin/cc
              export CXX=/usr/bin/c++
              export AR=/usr/bin/ar
              export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/cc

              export CC_aarch64_apple_ios=/usr/bin/clang
              export CXX_aarch64_apple_ios=/usr/bin/clang++
              export AR_aarch64_apple_ios=/usr/bin/ar
              export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER=/usr/bin/clang

              export CC_aarch64_apple_ios_sim=/usr/bin/clang
              export CXX_aarch64_apple_ios_sim=/usr/bin/clang++
              export AR_aarch64_apple_ios_sim=/usr/bin/ar
              export CARGO_TARGET_AARCH64_APPLE_IOS_SIM_LINKER=/usr/bin/clang

              export LLVM_TOOLS_DIR="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin"
              export LLVM_COV="$LLVM_TOOLS_DIR/llvm-cov"
              export LLVM_PROFDATA="$LLVM_TOOLS_DIR/llvm-profdata"
              export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner

              export PATH="$HOME/.bun/bin:$PATH"
              echo "Welcome to the Diaryx development environment!"
              echo "Targets enabled: x86_64-linux, aarch64-darwin, wasm32"
              echo "Rust: $(rustc --version)"
              echo "Coverage: $(cargo llvm-cov --version)"
            '';
          };
        });
    };
}
