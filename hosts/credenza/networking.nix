{ config, ... }:

{
  weblounge.publicIPv4 = "107.173.231.206";

  # RackNerd assigns a static address and runs no DHCP server. Matching the
  # NIC by MAC keeps this from depending on how the kernel names it.
  networking.useDHCP = false;
  networking.useNetworkd = true;
  networking.nameservers = [ "8.8.8.8" "8.8.4.4" ];
  # Under networkd, resolved is what applies networking.nameservers.
  services.resolved.enable = true;
  # Name lookups on the local link have nothing to find in a datacenter.
  services.resolved.settings.Resolve = {
    LLMNR = false;
    MulticastDNS = false;
  };

  systemd.network.networks."10-wan" = {
    matchConfig.MACAddress = "00:16:3c:1a:01:be";
    address = [ "${config.weblounge.publicIPv4}/25" ];
    routes = [{ Gateway = "107.173.231.129"; }];
    linkConfig.RequiredForOnline = "routable";
  };
}
