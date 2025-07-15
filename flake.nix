# FIXME: ~/.config/direnv/lib/hm-nix-direnv.sh depends on devshell-dir/env.bash but it isn't rooted
{
  description = "develop and build with nix";
  
  inputs = {
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    devshell = {
      url = "github:numtide/devshell";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    systems.url = "github:nix-systems/default-linux";
  };
  
  nixConfig = {
    allow-import-from-derivation = true;
    extra-substituters = "https://eigenvalue.cachix.org";
    extra-trusted-public-keys = "eigenvalue.cachix.org-1:ykerQDDa55PGxU25CETy9wF6uVDpadGGXYrFNJA3TUs=";
  };

  outputs = { nixpkgs, rust-overlay, devshell, systems, ... }: let
    inherit (nixpkgs.lib) genAttrs mapAttrs;
    systems' = import systems;
    pkgs = genAttrs systems' (system: import nixpkgs {
      inherit system;
      overlays = [devshell.overlays.default (import rust-overlay) (self: super: 
      assert !(super ? rust-toolchain); {
        rust-toolchain = super.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        rustfmt-nightly = super.rustfmt.override { asNightly = true; };
      })];
      config = {};
    });
  in {
    devShells = pkgs |> mapAttrs (system: pkgs': {
      default = pkgs'.devshell.mkShell ({ ... }: {
        imports = [
          "${devshell}/extra/language/c.nix"
        ];
        commands = with pkgs'; [
          { package = rust-toolchain; category = "rust"; }
          { package = rustfmt-nightly; category = "rust"; }
          { package = rusty-man; category = "rust"; }
          { package = mold; category = "build"; }
          { package = hyperfine; category = "debug"; }
        ];
      });
    });
  };
}
