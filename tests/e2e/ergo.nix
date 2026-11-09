{ vmTest, pkgs, ... }:

let
  # Ergo probes every push endpoint it's given, and only over HTTPS to a
  # public address; the client stands in for a push service at one.
  pushService = "push.test";
  pushAddress = "203.0.113.7";
  certificates = import ./lib/certificates.nix { inherit pkgs; } pushService;
in
vmTest {
  credenza = {
    security.pki.certificateFiles = [ "${certificates}/ca.pem" ];
    networking.hosts.${pushAddress} = [ pushService ];
    networking.interfaces.eth1.ipv4.routes = [{ address = pushAddress; prefixLength = 32; }];
  };

  client = {
    networking.interfaces.eth1.ipv4.addresses = [{ address = pushAddress; prefixLength = 32; }];
    networking.firewall.allowedTCPPorts = [ 443 ];
    services.nginx = {
      enable = true;
      virtualHosts.${pushService} = {
        onlySSL = true;
        sslCertificate = "${certificates}/${pushService}/cert.pem";
        sslCertificateKey = "${certificates}/${pushService}/key.pem";
        locations."/".return = "201";
      };
    };
  };
}
