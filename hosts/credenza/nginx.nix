{
  networking.firewall.allowedTCPPorts = [ 80 443 ];

  security.acme.acceptTerms = true;

  services.nginx = {
    enable = true;
    recommendedGzipSettings = true;
    recommendedOptimisation = true;
    recommendedProxySettings = true;
    recommendedTlsSettings = true;
    virtualHosts."weblounge.org" = {
      default = true;
      serverAliases = [
        "www.weblounge.org"
        "irc.weblounge.org"
        "credenza.weblounge.org"
      ];
      enableACME = true;
      forceSSL = true;
      root = ../../site;
    };
  };
}
