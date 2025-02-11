{
  description = "develop and build with nix";
  
  inputs = {
    crate2nix.url = "github:nix-community/crate2nix";
    rust-overlay.url = "github:oxalica/rust-overlay";
    systems.url = "github:nix-systems/default-linux";
  };
  
  nixConfig = {
    allow-import-from-derivation = true;
    extra-substituters = "https://eigenvalue.cachix.org";
    extra-trusted-public-keys = "eigenvalue.cachix.org-1:ykerQDDa55PGxU25CETy9wF6uVDpadGGXYrFNJA3TUs=";
  };

  outputs = { nixpkgs, crate2nix, rust-overlay, systems, ...}: let
    inherit (nixpkgs.lib) genAttrs mapAttrs;
    systems' = import systems;
    pkgs = genAttrs systems' (system: import nixpkgs {
      inherit system;
      overlays = [(import rust-overlay) (self: super: 
      assert !(super ? rust-toolchain); rec {
        rust-toolchain = super.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        # NOTE: for buildRustCrate/crate2nix
        rustc = rust-toolchain;
        cargo = rust-toolchain;
      }];
      config = {};
    });
    cargoNix = genAttrs systems' (system:
      crate2nix.tools.${system}.appliedCargoNix {
        name = "rustnix";
        src = ./.;
      }
    );
  in {
    checks = mapAttrs cargoNix (system: cargoNix':
      rustnix = cargoNix'.rootCrate.build.override {
        runTests = true;
      }
    );
    packages = mapAttrs cargoNix (system: cargoNix': {
      default = cargoNix'.rootCrate.bulid;
    );
    devShells = mapAttrs pkgs (system: pkgs': {
      default = with pkgs; mkShell {
        packages = [
          rust-toolchain
        ];
        shellHook = /* bash */ ''
        '';
      });
    };
  }
}
