{
  description = "NYC solar noon calculator";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        inherit (pkgs) lib rustPlatform;

        noon = rustPlatform.buildRustPackage {
          pname = "noon";
          version = "0.1.0";
          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          meta = with lib; {
            description = "NYC solar noon calculator";
            homepage = "https://github.com/jasonmc/noon";
            license = licenses.mit;
            mainProgram = "noon";
          };
        };
      in
      {
        packages.default = noon;

        checks = {
          noon = noon;
        };

        apps.default = {
          type = "app";
          program = lib.getExe noon;
          meta = noon.meta;
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            cargo-nextest
          ];
        };
      });
}
