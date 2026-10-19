{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;

  credenzaWith = modules: (weblounge.hosts.credenza.extendModules { inherit modules; }).config;

  conciergeExpiry = config: lib.filter (lib.hasPrefix "CONCIERGE_HISTORY_EXPIRE_TIME=")
    config.systemd.services.concierge.serviceConfig.Environment;

  ergoExpiry = config: config.weblounge.ergo.settings.history.restrictions.expire-time;
in
{
  testConciergeGoesByTheHistoryExpiryErgoUses = {
    expr = conciergeExpiry weblounge.hosts.credenza.config;
    expected = [ "CONCIERGE_HISTORY_EXPIRE_TIME=${ergoExpiry weblounge.hosts.credenza.config}" ];
  };

  testConciergeFollowsAChangeToErgosHistoryExpiry =
    let
      config = credenzaWith [{ weblounge.ergo.settings.history.restrictions.expire-time = lib.mkForce "7d"; }];
    in
    {
      expr = [ (ergoExpiry config) (conciergeExpiry config) ];
      expected = [ "7d" [ "CONCIERGE_HISTORY_EXPIRE_TIME=7d" ] ];
    };
}
