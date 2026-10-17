{ vmTest, ... }:

vmTest {
  credenza = { lib, ... }: {
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
