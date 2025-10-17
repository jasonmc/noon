{
  description = "NYC solar noon calculator";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        inherit (pkgs) lib;

        craneLib = crane.mkLib pkgs;
        src = craneLib.cleanCargoSource (craneLib.path ./.);

        commonArgs = {
          pname = "noon";
          version = "0.1.0";
          inherit src;
          cargoLock = ./Cargo.lock;
          strictDeps = true;
          buildInputs = [
            pkgs.cargo-nextest
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        noon = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });
      in
      {
        checks = {
          inherit noon;
          noon-nextest = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
          });
        };

        packages.default = noon;

        apps.default = flake-utils.lib.mkApp {
          drv = noon;
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
        };
      });
}
