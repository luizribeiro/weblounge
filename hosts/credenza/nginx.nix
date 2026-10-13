{
  networking.firewall.allowedTCPPorts = [ 80 ];

  services.nginx = {
    enable = true;
    recommendedGzipSettings = true;
    recommendedOptimisation = true;
    recommendedProxySettings = true;
    virtualHosts."weblounge.org" = {
      default = true;
      serverAliases = [ "irc.weblounge.org" ];
      root = ../../site;
    };
  };
}
