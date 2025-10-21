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
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };
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

        # Android build helper function using Nix cross-compilation
        mkAndroidPackage =
          androidPkgs: targetName:
          androidPkgs.rustPlatform.buildRustPackage {
            pname = "${crateName}-android-${targetName}";
            version = crateVersion;
            src = lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoHash = lib.fakeSha256;

            nativeBuildInputs = [ ];
            buildInputs = [ ];

            # Disable tests for cross-compilation
            doCheck = false;

            meta = with lib; {
              description = "Small helper to launch applications with custom rules (Android ${targetName})";
              license = licenses.mit;
              maintainers = [ ];
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
          android-aarch64 = mkAndroidPackage pkgs.pkgsCross.aarch64-android-prebuilt "aarch64";
          android-armv7 = mkAndroidPackage pkgs.pkgsCross.armv7a-android-prebuilt "armv7";
          android-x86_64 = mkAndroidPackage pkgs.pkgsCross.x86_64-android-prebuilt "x86_64";
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
