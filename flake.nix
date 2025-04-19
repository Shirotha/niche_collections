{
  description = "develop and build with nix";
  
  inputs = {
    crate2nix.url = "github:nix-community/crate2nix";
    rust-overlay.url = "github:oxalica/rust-overlay";
    devshell.url = "github:numtide/devshell";
    systems.url = "github:nix-systems/default-linux";
  };
  
  nixConfig = {
    allow-import-from-derivation = true;
    extra-substituters = "https://eigenvalue.cachix.org";
    extra-trusted-public-keys = "eigenvalue.cachix.org-1:ykerQDDa55PGxU25CETy9wF6uVDpadGGXYrFNJA3TUs=";
  };

  outputs = { nixpkgs, crate2nix, rust-overlay, devshell, systems, ... }: let
    inherit (nixpkgs.lib) genAttrs mapAttrs;
    systems' = import systems;
    pkgs = genAttrs systems' (system: import nixpkgs {
      inherit system;
      overlays = [devshell.overlays.default (import rust-overlay) (self: super: 
      assert !(super ? rust-toolchain); {
        rust-toolchain = super.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        rustfmt-nightly = (super.rust-bin.selectLatestNightlyWith (tc: tc.rustfmt));
      })];
      config = {};
    });
    cargoNix = genAttrs systems' (system:
      crate2nix.tools.${system}.appliedCargoNix {
        name = "rustnix";
        src = ./.;
      }
    );
  in {
    checks = cargoNix |> mapAttrs (system: cargoNix': {
      rustnix = cargoNix'.rootCrate.build.override {
        runTests = true;
      };
    });
    packages = cargoNix |> mapAttrs (system: cargoNix': let
      pkgs' = pkgs.${system};
    in rec {
      default = rustnix;
      rustnix = cargoNix'.rootCrate.build;
      inherit (pkgs') rust-toolchain;
      rust-toolchain-versions = pkgs'.writeScriptBin "rust-toolchain-versions" /* bash */ ''
        ${rust-toolchain}/bin/cargo --version
        ${rust-toolchain}/bin/rustc --version
      '';
    });
    devShells = pkgs |> mapAttrs (system: pkgs': {
      default = pkgs'.devshell.mkShell ({ ... }: {
        imports = [
          "${devshell}/extra/language/c.nix"
        ];
        commands = with pkgs'; [
          { package = rust-toolchain; category = "rust"; }
          { package = rustfmt-nightly; category = "rust"; }
          { package = mold; category = "build"; }
        ];
      });
    });
  };
}
