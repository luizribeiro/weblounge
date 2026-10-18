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
      # A vacuum waits for what its owner waits for, such as the unit that
      # creates and migrates the database.
      inherit (config.systemd.services.${service}) after;
      startAt = "hourly";
      serviceConfig = config.weblounge.sandbox // lib.filterAttrs (key: _: lib.elem key shared) owner // {
        Type = "oneshot";
        PrivateNetwork = true;
        RestrictAddressFamilies = "none";
        User = owner.User or service;
        # sqlite3 would create a missing database as this unit's user, with a
        # mode its owner may not open. One that doesn't exist holds no
        # deleted data, so there's nothing to fail for.
        ExecStart = pkgs.writeShellScript "${service}-vacuum" ''
          [[ -e ${database} ]] || { echo "<4>${database} is missing, so there is nothing to vacuum"; exit 0; }
          checkpoint=$(${lib.getExe pkgs.sqlite} -cmd '.timeout 10000' 'file:${database}?mode=rw' \
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
