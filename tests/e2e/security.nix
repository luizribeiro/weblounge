{ vmTest, ... }:

let
  # Addresses outside the private ranges on the test network, standing in for
  # servers on the Internet such as MXroute's.
  public = { credenza = "198.51.100.1"; client = "198.51.100.2"; };
  onTheInternet = address: {
    networking.interfaces.eth1.ipv4.addresses = [{ inherit address; prefixLength = 24; }];
  };
in
vmTest {
  credenza = { config, lib, pkgs, ... }: {
    imports = [ (onTheInternet public.credenza) ];
    environment.systemPackages = [
      pkgs.nftables
      # Runs a command confined the way the mailer is.
      (pkgs.writeShellScriptBin "as-mailer" ''
        exec systemd-run --quiet --pipe --wait --collect --service-type=exec --setenv=PATH="$PATH" \
          ${lib.escapeShellArgs (config.weblounge.runProperties
            (removeAttrs config.systemd.services.concierge-mailer.serviceConfig [ "ExecStart" "Restart" ]))} \
          "$@"
      '')
    ];
  };

  client = {
    imports = [ (onTheInternet public.client) ];
    networking.firewall.allowedTCPPorts = [ 80 443 465 ];
  };
}
