{ config, lib, pkgs, ... }:

let
  unit = "weblounge-secrets";
  directory = "/var/lib/${unit}";
  inherit (config.weblounge) secrets;

  secretOptions = { name, ... }: {
    options = {
      path = lib.mkOption {
        type = lib.types.str;
        readOnly = true;
        default = "${directory}/${name}";
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
      ${service} = {
        requires = [ "${unit}.service" ];
        after = [ "${unit}.service" ];
        serviceConfig.LoadCredential = [ "${name}:${secret.path}" ];
      };
    })
    secret.loadedBy;
in
{
  options.weblounge.secrets = lib.mkOption {
    type = lib.types.attrsOf (lib.types.submodule secretOptions);
    default = { };
    description = "Random secrets generated on the machine itself, so they never enter the Nix store.";
  };

  config.systemd.services = lib.mkMerge ([{
    ${unit} = {
      description = "Generate this machine's secrets";
      path = [ pkgs.openssl ];
      script = lib.concatMapStrings generate (lib.attrValues secrets);
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        StateDirectory = unit;
        StateDirectoryMode = "0700";
        UMask = "0077";
      };
    };
  }] ++ lib.concatLists (lib.mapAttrsToList loadInto secrets));
}
