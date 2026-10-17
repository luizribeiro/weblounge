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
  credenza = { config, lib, pkgs, ... }:
    let
      # as-<unit> runs a command confined the way the unit is.
      confinedAs = unit: pkgs.writeShellScriptBin "as-${unit}" ''
        exec systemd-run --quiet --pipe --wait --collect --service-type=exec --setenv=PATH="$PATH" \
          ${lib.escapeShellArgs (config.weblounge.runProperties
            (removeAttrs config.systemd.services.${unit}.serviceConfig [ "ExecStart" "Restart" ]))} \
          "$@"
      '';
    in
    {
      imports = [ (onTheInternet public.credenza) ];
      environment.systemPackages = [ pkgs.nftables ] ++ map confinedAs [ "concierge" "concierge-mailer" ];
    };

  client = {
    imports = [ (onTheInternet public.client) ];
    networking.firewall.allowedTCPPorts = [ 80 443 465 ];
  };
}
