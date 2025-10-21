{
  description = "Nix flake for the openit command-line tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;
        cargoToml = lib.importTOML ./Cargo.toml;
        crateName = cargoToml.package.name;
        crateVersion = cargoToml.package.version;
        nativeBuildInputs = with pkgs; [ pkg-config ];

        # GTK4 dependencies only needed for icon-picker feature
        buildInputs = with pkgs; [
          openssl
        ];

        # Base package without icon-picker
        cratePackage = pkgs.rustPlatform.buildRustPackage {
          pname = crateName;
          version = crateVersion;
          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoHash = lib.fakeSha256;
          inherit nativeBuildInputs;
          buildInputs = [ ];
          meta = with lib; {
            description = "Small helper to launch applications with custom rules";
            license = licenses.mit;
            maintainers = [ ];
          };
        };

        # Package with icon-picker feature enabled
        cratePackageWithIconPicker = pkgs.rustPlatform.buildRustPackage {
          pname = "${crateName}-with-icon-picker";
          version = crateVersion;
          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoHash = lib.fakeSha256;
          buildFeatures = [ "icon-picker" ];
          inherit nativeBuildInputs;
          buildInputs = buildInputs;
          meta = with lib; {
            description = "Small helper to launch applications with custom rules (with icon-picker)";
            license = licenses.mit;
            maintainers = [ ];
          };
        };

        # Cross-compilation helper function
        mkCrossPackage =
          crossPkgs: targetName:
          crossPkgs.rustPlatform.buildRustPackage {
            pname = "${crateName}-${targetName}";
            version = crateVersion;
            src = lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoHash = lib.fakeSha256;

            # Don't include pkg-config for cross-compilation as it often fails
            # and isn't needed for static Rust binaries
            nativeBuildInputs = [ ];
            buildInputs = [ ];

            meta = with lib; {
              description = "Small helper to launch applications with custom rules (${targetName})";
              license = licenses.mit;
              maintainers = [ ];
            };
          };

        # Android build helper function
        mkAndroidPackage =
          target: targetName:
          pkgs.stdenv.mkDerivation {
            pname = "${crateName}-android-${targetName}";
            version = crateVersion;
            src = lib.cleanSource ./.;

            nativeBuildInputs =
              with pkgs;
              [
                cargo
                rustc
                cargo-ndk
                rustup
                pkg-config
              ]
              ++ lib.optionals pkgs.stdenv.isLinux [
                pkgs.androidenv.androidPkgs.ndk-bundle
              ];

            buildInputs = [ pkgs.openssl ];

            buildPhase = ''
              export CARGO_HOME=$(mktemp -d)
              # Add Android target
              rustup target add ${target} || true

              # Build with cargo-ndk
              cargo ndk --target ${target} --platform 21 -- build --release
            '';

            installPhase = ''
              mkdir -p $out/bin
              # Copy the binary (not a .so for CLI tools)
              if [ -f target/${target}/release/${crateName} ]; then
                cp target/${target}/release/${crateName} $out/bin/
              fi
            '';

            meta = with lib; {
              description = "Small helper to launch applications with custom rules (Android ${targetName})";
              license = licenses.mit;
              maintainers = [ ];
              platforms = [
                "x86_64-linux"
                "aarch64-linux"
              ];
            };
          };

      in
      {
        packages = {
          default = cratePackage;
          with-icon-picker = cratePackageWithIconPicker;

          # Cross-platform builds
          # Windows
          windows-x86_64 = mkCrossPackage pkgs.pkgsCross.mingwW64 "windows-x86_64";

          # macOS
          macos-aarch64 = mkCrossPackage pkgs.pkgsCross.aarch64-darwin "macos-aarch64";
          macos-x86_64 = mkCrossPackage pkgs.pkgsCross.x86_64-darwin "macos-x86_64";

          # Linux
          linux-x86_64 = mkCrossPackage pkgs.pkgsCross.gnu64 "linux-x86_64";
          linux-aarch64 = mkCrossPackage pkgs.pkgsCross.aarch64-multiplatform "linux-aarch64";

          # Android builds for common architectures
          android-aarch64 = mkAndroidPackage "aarch64-linux-android" "aarch64";
          android-armv7 = mkAndroidPackage "armv7-linux-androideabi" "armv7";
          android-x86_64 = mkAndroidPackage "x86_64-linux-android" "x86_64";
        };

        apps.default = {
          type = "app";
          program = "${cratePackage}/bin/${crateName}";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer
            cargo-edit
            cargo-deny
            cargo-audit
            cargo-tarpaulin
            cargo-ndk
            rustup
          ];

          # Include GTK4 in dev shell for icon-picker development
          buildInputs = buildInputs;
          inherit nativeBuildInputs;
        };

        formatter = pkgs.alejandra;

        checks.build = cratePackage;
      }
    );
}
