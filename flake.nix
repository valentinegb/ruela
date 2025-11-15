{
  inputs = {
    # Change to NixOS/nixpkgs/nixos-unstable after rust 1.91.1 leaves staging
    nixpkgs.url = "github:tvbeat/nixpkgs/rust191-1";
  };

  outputs =
    { self, nixpkgs }:
    {

      packages =
        nixpkgs.lib.genAttrs (nixpkgs.lib.remove "x86_64-freebsd" nixpkgs.lib.systems.flakeExposed)
          (system: {
            ruela = nixpkgs.legacyPackages.${system}.rustPlatform.buildRustPackage {
              pname = "ruela";
              version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
              src = ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
                allowBuiltinFetchGit = true;
              };
              meta.mainProgram = "ruela";
            };

            default = self.packages.${system}.ruela;
          });

      nixosModules = {
        ruela =
          {
            lib,
            config,
            pkgs,
            ...
          }:
          {
            options = {
              services.ruela = {
                enable = lib.mkEnableOption "ruela";
                token = lib.mkOption { type = lib.types.str; };
              };
            };
            config = lib.mkIf config.services.ruela.enable {
              systemd.services.ruela = {
                wantedBy = [ "multi-user.target" ];
                after = [ "network.target" ];
                environment.RUELA_DISCORD_TOKEN = config.services.ruela.token;
                serviceConfig = {
                  ExecStart = lib.getExe self.packages.${pkgs.system}.ruela;
                  Restart = "always";
                };
              };
            };
          };

        default = self.nixosModules.ruela;
      };

    };
}
