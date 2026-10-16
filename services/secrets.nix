{ config, lib, pkgs, ... }:

let
  outerConfig = config;
  unit = "weblounge-secrets";
  directory = "/var/lib/${unit}";
  inherit (config.weblounge) secrets;
  generated = lib.filterAttrs (_: secret: !secret.fromSops) secrets;
  fromSops = lib.filterAttrs (_: secret: secret.fromSops) secrets;

  secretOptions = { name, config, ... }: {
    options = {
      fromSops = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Whether the secret comes from outside, encrypted in secrets/<host>.yaml
          under its name, rather than generated on the machine.
        '';
      };
      path = lib.mkOption {
        type = lib.types.str;
        readOnly = true;
        default = if config.fromSops then outerConfig.sops.secrets.${name}.path else "${directory}/${name}";
      };
      loadedBy = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "Services that get this secret as a systemd credential named after it.";
      };
    };
  };

  generate = secret: ''
    if [ ! -e ${secret.path} ]; then
      openssl rand -hex 32 > ${secret.path}.new
      mv ${secret.path}.new ${secret.path}
    fi
  '';

  loadInto = name: secret: map
    (service: {
      ${service} = lib.optionalAttrs (!secret.fromSops)
        {
          requires = [ "${unit}.service" ];
          after = [ "${unit}.service" ];
        } // {
        serviceConfig.LoadCredential = [ "${name}:${secret.path}" ];
      };
    })
    secret.loadedBy;
in
{
  options.weblounge.secrets = lib.mkOption {
    type = lib.types.attrsOf (lib.types.submodule secretOptions);
    default = { };
    description = "Secrets that never enter the Nix store: random ones generated on the machine, or ones from sops.";
  };

  config = {
    sops = lib.mkIf (fromSops != { }) {
      defaultSopsFile = lib.mkDefault ../secrets/${config.networking.hostName}.yaml;
      secrets = lib.mapAttrs (_: _: { }) fromSops;
    };

    systemd.services = lib.mkMerge ([{
      ${unit} = {
        description = "Generate this machine's secrets";
        path = [ pkgs.openssl ];
        script = lib.concatMapStrings generate (lib.attrValues generated);
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          StateDirectory = unit;
          StateDirectoryMode = "0700";
          UMask = "0077";
        };
      };
    }] ++ lib.concatLists (lib.mapAttrsToList loadInto secrets));
  };
}
