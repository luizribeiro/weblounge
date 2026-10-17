# Deleting a row leaves its bytes in SQLite's free pages and write-ahead
# log; vacuuming and truncating the log is what takes deleted data off the
# disk. Each database is vacuumed hourly as the service that owns it.
{ config, lib, pkgs, ... }:

let
  shared = [ "DynamicUser" "StateDirectory" "StateDirectoryMode" "SupplementaryGroups" "ReadWritePaths" "UMask" ];

  vacuum = service: database:
    let owner = config.systemd.services.${service}.serviceConfig;
    in lib.nameValuePair "${service}-vacuum" {
      description = "Clear deleted data out of ${database}";
      startAt = "hourly";
      serviceConfig = config.weblounge.sandbox // lib.filterAttrs (key: _: lib.elem key shared) owner // {
        Type = "oneshot";
        PrivateNetwork = true;
        RestrictAddressFamilies = "none";
        User = owner.User or service;
        ExecStart = pkgs.writeShellScript "${service}-vacuum" ''
          checkpoint=$(${lib.getExe pkgs.sqlite} -cmd '.timeout 10000' ${database} \
            'VACUUM' 'PRAGMA wal_checkpoint(TRUNCATE)') || exit
          [[ $checkpoint == 0\|* ]] || { echo "the write-ahead log was busy: $checkpoint" >&2; exit 1; }
        '';
      };
    };
in
{
  imports = [ ./sandbox.nix ];

  options.weblounge.sqliteVacuum = lib.mkOption {
    type = lib.types.attrsOf lib.types.str;
    default = { };
    description = "SQLite databases to vacuum hourly, by the service that owns them.";
  };

  config.systemd.services = lib.mapAttrs' vacuum config.weblounge.sqliteVacuum;
}
