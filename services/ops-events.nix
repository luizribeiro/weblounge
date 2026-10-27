# The host's major events, one plain line each in #ops, through bender's
# louro: deploys and rollbacks, boots and shutdowns, and the key services'
# crashes, failures and recoveries. ops-notify posts a line of root's.
{ config, lib, pkgs, ... }:

let
  opsEvents = pkgs.callPackage ../pkgs/ops-events {
    louro = config.services.louro.package;
    inherit (config.services.louro) socket;
    host = config.networking.hostName;
  };
  opsEvent = lib.getExe opsEvents;
  queue = "/run/ops-events/queue";

  keyServices = [ "ergochat" "concierge" "nginx" "concierge-mailer" ];
  certificateUnits = lib.concatMap
    (cert: [ "acme-${cert}" "acme-order-renew-${cert}" ])
    (lib.attrNames config.security.acme.certs);

  reportsFailure.onFailure = [ "ops-failed@%N.service" ];
  # Without an order, so it looks for a deploy while the switch that
  # (re)started the service still runs. Not in serviceConfig, which
  # concierge-admin and the mailer's check copy.
  reportsStart = name: { wants = [ "ops-started@${name}.service" ]; };

  # What may post to #ops: louro's group, and nothing else.
  poster = config.weblounge.sandbox // {
    DynamicUser = true;
    User = "ops-notify";
    SupplementaryGroups = [ config.services.louro.group ];
    PrivateNetwork = true;
    # louro's socket and systemd's bus.
    RestrictAddressFamilies = [ "AF_UNIX" ];
    # /proc/locks tells when a deploy is done.
    ProcSubset = "all";
  };
in
{
  imports = [ ./bender.nix ./sandbox.nix ];

  environment.systemPackages = [ opsEvents.ops-notify ];

  systemd.tmpfiles.rules = [
    "d /run/ops-events 0755 root root -"
    "d /run/ops-events/failed 0755 root root -"
    "d /run/ops-events/switches 0755 root root -"
    # The hooks queue events as root; ops-post takes them off.
    "d ${queue} 0770 root ${config.services.louro.group} -"
  ];

  # Again whenever ops-post stops with events left.
  systemd.paths.ops-post = {
    wantedBy = [ "paths.target" ];
    pathConfig.DirectoryNotEmpty = queue;
  };

  system.preSwitchChecks.ops-events = ''${opsEvent} switch "$1" "$2" || true'';

  systemd.services = lib.mkMerge [
    (lib.genAttrs keyServices (name: reportsFailure // reportsStart name))
    (lib.genAttrs certificateUnits (_: reportsFailure))
    {
      "ops-started@" = {
        description = "Tell #ops of %i's start, unless a boot or deploy covers it";
        restartIfChanged = false;
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${opsEvent} started %i";
          TimeoutStartSec = "6min";
        };
      };

      "ops-failed@" = {
        description = "Tell #ops %i failed, unless a restart follows";
        restartIfChanged = false;
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${opsEvent} failed %i";
        };
      };

      # One run at a time, so the lines go out in the order queued.
      ops-post = {
        description = "Post the host's queued events to #ops";
        # A deploy that changes it leaves the run waiting to post alone.
        restartIfChanged = false;
        serviceConfig = poster // {
          # Simple, so the boot it waits on can finish.
          Type = "simple";
          ExecStart = "${opsEvent} drain";
          ReadWritePaths = [ queue ];
        };
      };

      ops-lifecycle = {
        description = "Post the host's boots and shutdowns to #ops";
        wantedBy = [ "multi-user.target" ];
        wants = [ "network-online.target" ];
        # Stops before louro and the network, so the last line goes out.
        after = [ "louro.service" "network-online.target" ];
        # Stopping it says the host shut down cleanly, which a deploy didn't.
        restartIfChanged = false;
        # It posts the shutdown itself: no unit can start while one runs.
        serviceConfig = poster // {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "+${opsEvent} boot";
          ExecStop = "${opsEvent} shutdown";
          ExecStopPost = "+${opsEvent} down";
          StateDirectory = "ops-events";
          TimeoutStopSec = 30;
        };
      };
    }
  ];
}
