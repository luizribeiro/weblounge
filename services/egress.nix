# Confines a service's outbound connections to a few TCP ports, wherever
# they lead, and name lookups through resolved's stub on loopback. A rule
# naming a unit's cgroup or user resolves it as the rule loads, which is
# before the unit runs, so systemd puts each unit's cgroup in an nftables
# set as it starts (NFTSet=), and the rules match the set.
{ config, lib, pkgs, ... }:

let
  unit = "weblounge-egress";
  table = "weblounge_egress";
  nft = lib.getExe pkgs.nftables;
  services = config.weblounge.egressPorts;
  # systemd takes no dashes in the names NFTSet= refers to.
  setOf = service: lib.replaceStrings [ "-" ] [ "_" ] service;

  # The table is this unit's alone, so reloading the firewall leaves the sets
  # systemd filled; it's replaced whole, so loading it twice is harmless.
  ruleset = pkgs.writeText "${unit}.nft" ''
    destroy table inet ${table}
    table inet ${table} {
      ${lib.concatStrings (lib.mapAttrsToList (service: ports: ''
        set ${setOf service} {
          type cgroupsv2
        }

        chain ${setOf service} {
          tcp dport { ${lib.concatMapStringsSep ", " toString ports} } accept
          ip daddr 127.0.0.53 meta l4proto { tcp, udp } th dport 53 accept
          reject
        }
      '') services)}
      # Level 2 is system.slice/<unit>, so a confined unit can't set Slice=.
      chain output {
        type filter hook output priority filter; policy accept;
        ${lib.concatMapStrings (service: ''
          socket cgroupv2 level 2 @${setOf service} jump ${setOf service}
        '') (lib.attrNames services)}
      }
    }
  '';

  confine = service: _: {
    # Bound, so the service stops rather than run unconfined when the table
    # goes. A deploy that replaces the table restarts the service too, into
    # the new set, as its unit changes with the ruleset.
    bindsTo = [ "${unit}.service" ];
    after = [ "${unit}.service" ];
    restartTriggers = [ ruleset ];
    serviceConfig.NFTSet = "cgroup:inet:${table}:${setOf service}";
  };
in
{
  options.weblounge.egressPorts = lib.mkOption {
    type = lib.types.attrsOf (lib.types.nonEmptyListOf lib.types.port);
    default = { };
    description = "Services, by unit name, and the only TCP ports each may connect to.";
  };

  config = lib.mkIf (services != { }) {
    systemd.services = lib.mapAttrs confine services // {
      ${unit} = {
        description = "Confine services' outbound connections";
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${nft} -f ${ruleset}";
          ExecStop = "${nft} destroy table inet ${table}";
        };
      };
    };
  };
}
