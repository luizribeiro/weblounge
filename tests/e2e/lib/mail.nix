# What `mail = true;` adds to a VM test: a mail node running Mailpit, which
# takes credenza's mail the way MXroute does, over implicit TLS on 465 with
# a login, and a certificate from a test CA that credenza trusts. Tests read
# what arrived through Mailpit's API with the mail helpers in helpers.py.
{ pkgs }:

let
  server = "mail";
  # secrets/vm.yaml's smtp-password, which the mailer logs in with.
  password = "vm-smtp-password";
  certificates = pkgs.runCommand "mail-test-certificates" { nativeBuildInputs = [ pkgs.minica ]; } ''
    mkdir $out && cd $out && minica --domains ${server}
  '';
  logins = username: pkgs.runCommand "mailpit-logins" { nativeBuildInputs = [ pkgs.apacheHttpd ]; } ''
    htpasswd -nbB ${pkgs.lib.escapeShellArgs [ username password ]} > $out
  '';
in
{
  node = { nodes, ... }: {
    services.mailpit.instances.test = {
      smtp = "[::]:465";
      smtp-require-tls = true;
      smtp-tls-cert = "${certificates}/${server}/cert.pem";
      smtp-tls-key = "${certificates}/${server}/key.pem";
      smtp-auth-file = "${logins nodes.credenza.systemd.services.concierge-mailer.environment.CONCIERGE_SMTP_USERNAME}";
    };
    systemd.services.mailpit-test.serviceConfig.AmbientCapabilities = "CAP_NET_BIND_SERVICE";
    networking.firewall.allowedTCPPorts = [ 465 ];
    environment.systemPackages = [ pkgs.curl ];
  };

  credenza = { nodes, ... }: {
    security.pki.certificateFiles = [ "${certificates}/minica.pem" ];
    weblounge.mail.server = server;
    # The test network is a private range, which the mailer may not reach.
    systemd.services.concierge-mailer.serviceConfig.IPAddressAllow = [ nodes.${server}.networking.primaryIPAddress ];
  };
}
