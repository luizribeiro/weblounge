{ vmTest, ... }:

vmTest {
  credenza = {
    # What a deploy of a commit that changes concierge switches credenza to.
    specialisation.deployed.configuration = {
      system.configurationRevision = "0123456789abcdef0123456789abcdef01234567";
      systemd.services.concierge.serviceConfig.Environment = [ "CONCIERGE_DEPLOYED_AGAIN=1" ];
    };
  };
}
