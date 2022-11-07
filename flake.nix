{
  inputs = {
    cargo2nix.url = "github:cargo2nix/cargo2nix";
    flake-utils.follows = "cargo2nix/flake-utils";
    nixpkgs.follows = "cargo2nix/nixpkgs";
  };

  outputs = inputs: with inputs;
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [cargo2nix.overlays.default];
        };

        rustPkgs = pkgs.rustBuilder.makePackageSet {
          rustVersion = "1.61.0";
          packageFun = import ./Cargo.nix;
        };

      in rec {
        packages = {
          erooster = (rustPkgs.workspace.erooster {}).bin;
          eroosterctl = (rustPkgs.workspace.eroosterctl {}).bin;
          erooster_core = (rustPkgs.workspace.erooster_core {}).lib;
          erooster_imap = (rustPkgs.workspace.erooster_imap {}).lib;
          erooster_smtp = (rustPkgs.workspace.erooster_smtp {}).lib;
          erooster_web = (rustPkgs.workspace.erooster_web {}).lib;
          default = packages.erooster;
        };
      }
    );
}
