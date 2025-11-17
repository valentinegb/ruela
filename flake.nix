{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
  };

  outputs =
    { self, nixpkgs }:
    {

      packages =
        nixpkgs.lib.genAttrs (nixpkgs.lib.remove "x86_64-freebsd" nixpkgs.lib.systems.flakeExposed)
          (system: {
            ruela = nixpkgs.legacyPackages.${system}.rustPackages_1_88.rustPlatform.buildRustPackage {
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
