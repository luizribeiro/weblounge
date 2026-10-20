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
      # as-<unit> runs a command confined the way the unit, or the template
      # unit <unit>@, is, from the store, as a unit may not see
      # /run/current-system.
      bins = lib.makeBinPath [ pkgs.bash pkgs.coreutils pkgs.getent ];
      unitOnly = [ "ExecStart" "ExecStopPost" "Restart" "StandardInput" "StandardOutput" ];
      # systemd-run can't set RestrictAddressFamilies=none; the subtests
      # read it off the unit with systemd-analyze instead.
      runnable = lib.filterAttrs (name: value: name != "RestrictAddressFamilies" || value != "none");
      confinedAs = unit: pkgs.writeShellScriptBin "as-${lib.removeSuffix "@" unit}" ''
        PATH=${bins}:$PATH
        exec systemd-run --quiet --pipe --wait --collect --service-type=exec --setenv=PATH=${bins} \
          ${lib.escapeShellArgs (config.weblounge.runProperties
            (runnable (removeAttrs config.systemd.services.${unit}.serviceConfig unitOnly)))} \
          "$(command -v "$1")" "''${@:2}"
      '';
    in
    {
      imports = [ (onTheInternet public.credenza) ];
      environment.systemPackages = [ pkgs.nftables ]
        ++ map confinedAs [ "concierge" "concierge-mailer" "concierge-scrubber@" ];
    };

  client = {
    imports = [ (onTheInternet public.client) ];
    networking.firewall.allowedTCPPorts = [ 80 443 465 ];
  };
}
