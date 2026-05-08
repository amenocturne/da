{
  description = "da - yes. — classify a bash command as approve/defer/deny under explicit policies";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      cargoToml = nixpkgs.lib.importTOML ./Cargo.toml;

      mkDa =
        pkgs:
        let
          inherit (pkgs) lib;
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;

          src = lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              let
                base = baseNameOf (toString path);
              in
              !(lib.hasSuffix ".png" base)
              && !(lib.hasSuffix ".jpg" base)
              && !(lib.hasSuffix ".jpeg" base)
              && base != "target"
              && base != "tmp";
          };

          cargoLock.lockFile = ./Cargo.lock;

          # Tests are hermetic: pure parsers, no PTYs, no network, no
          # writable-HOME requirements. Safe to run inside the nix sandbox.
          doCheck = true;

          meta = {
            description = cargoToml.package.description;
            homepage = cargoToml.package.repository;
            license = lib.licenses.mit;
            mainProgram = "da";
            platforms = lib.platforms.unix;
          };
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          da = mkDa nixpkgs.legacyPackages.${system};
        in
        {
          default = da;
          da = da;
        }
      );

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/da";
        };
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.default ];
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.rust-analyzer
            ];
          };
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt-rfc-style);
    };
}
