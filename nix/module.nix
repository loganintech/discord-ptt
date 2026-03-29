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
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    # Ensure the user can read input devices
    users.groups.input = {};

    systemd.user.services.discord-ptt = {
      description = "Discord Push-to-Talk";
      after = [ "graphical-session.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/discord-ptt";
        Restart = "on-failure";
        RestartSec = 3;
      };
    };
  };
}
