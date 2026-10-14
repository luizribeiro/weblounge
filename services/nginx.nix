{
  networking.firewall.allowedTCPPorts = [ 80 443 ];

  security.acme.acceptTerms = true;

  services.nginx = {
    enable = true;
    recommendedGzipSettings = true;
    recommendedOptimisation = true;
    recommendedProxySettings = true;
    recommendedTlsSettings = true;
    # We promise members that no IP addresses are kept anywhere.
    commonHttpConfig = ''
      log_format anonymous '[$time_local] $host "$request" $status $body_bytes_sent '
                           '"$http_referer" "$http_user_agent"';
      access_log /var/log/nginx/access.log anonymous;
    '';
    # Errors about a request carry "client: <ip>" at any level; emerg is
    # left with little but failures to start.
    logError = "stderr emerg";
  };
}
