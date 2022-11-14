{
  inputs = {
    cargo2nix.url = "github:cargo2nix/cargo2nix/unstable";
    flake-utils.follows = "cargo2nix/flake-utils";
    nixpkgs.follows = "cargo2nix/nixpkgs";
  };

  outputs = inputs:
    with inputs;
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ cargo2nix.overlays.default ];
        };

        rustPkgs = pkgs.rustBuilder.makePackageSet {
          #rustVersion = "1.64.0";
          rustChannel = "nightly";
          packageFun = import ./Cargo.nix;
          # Use the existing all list of overrides and append your override
          packageOverrides = pkgs:
            pkgs.rustBuilder.overrides.all ++ [

              # parentheses disambiguate each makeOverride call as a single list element
              (pkgs.rustBuilder.rustLib.makeOverride {
                name = "vergen";
                overrideAttrs = drv: {
                  buildInputs = drv.buildInputs or [ ] ++ [ pkgs.git ];
                };
              })
            ];
        };

      in rec {
        packages = {
          erooster = (rustPkgs.workspace.erooster { });
          eroosterctl = (rustPkgs.workspace.eroosterctl { });
          default = packages.erooster;
        };
      });
}
