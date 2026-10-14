{ vmTest, ... }:

vmTest {
  # What a deploy that changes concierge switches credenza to.
  credenza.specialisation.changed-concierge.configuration = {
    systemd.services.concierge.serviceConfig.Environment = [ "CONCIERGE_DEPLOYED_AGAIN=1" ];
  };
}
