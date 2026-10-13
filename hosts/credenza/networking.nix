{
  # RackNerd assigns a static address and runs no DHCP server. Matching the
  # NIC by MAC keeps this from depending on how the kernel names it.
  networking.useDHCP = false;
  networking.useNetworkd = true;
  networking.nameservers = [ "8.8.8.8" "8.8.4.4" ];

  systemd.network.networks."10-wan" = {
    matchConfig.MACAddress = "00:16:3c:1a:01:be";
    address = [ "107.173.231.206/25" ];
    routes = [{ Gateway = "107.173.231.129"; }];
    linkConfig.RequiredForOnline = "routable";
  };
}
