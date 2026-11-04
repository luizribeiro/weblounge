{ vmTest, ... }:

vmTest {
  browsers.bots-browser = ./clients/bots-browser.py;
  credenza = { lib, ... }: {
    # On credenza, luiz founded #lounge and banned bots' hosts before
    # bender existed; the test starts bender once it has done the same.
    systemd.services.louro.wantedBy = lib.mkForce [ ];
    # What a deploy that changes concierge switches credenza to.
    specialisation.changed-concierge.configuration = {
      systemd.services.concierge.serviceConfig.Environment = [ "CONCIERGE_DEPLOYED_AGAIN=1" ];
    };
    # credenza from before its secrets moved to RAM, to deploy from and back to.
    specialisation.secrets-on-disk.configuration = {
      weblounge.secrets = lib.genAttrs [ "ergo-api-token" "oauth-introspection-secret" "bender-password" ]
        (_: { ephemeral = lib.mkForce false; });
    };
  };
}
