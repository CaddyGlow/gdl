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

      in
      {
        packages = {
          default = cratePackage;
          with-icon-picker = cratePackageWithIconPicker;
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
