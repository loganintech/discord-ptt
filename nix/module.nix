{ config, lib, pkgs, ... }:

let
  cfg = config.services.discord-ptt;
in
{
  options.services.discord-ptt = {
    enable = lib.mkEnableOption "Discord push-to-talk daemon";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The discord-ptt package to use.";
    };

    credentialsFile = lib.mkOption {
      type = lib.types.path;
      description = ''
        Path to an EnvironmentFile containing DISCORD_PTT_CLIENT_ID and
        DISCORD_PTT_CLIENT_SECRET, one per line in KEY=VALUE format.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    users.groups.input = {};

    systemd.user.services.discord-ptt = {
      description = "Discord Push-to-Talk";
      after = [ "graphical-session.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/discord-ptt";
        EnvironmentFile = cfg.credentialsFile;
        Restart = "on-failure";
        RestartSec = 3;
      };
    };
  };
}
