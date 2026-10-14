{ lib, ... }:

let
  contentSecurityPolicy = lib.concatStringsSep "; " [
    "default-src 'none'"
    "script-src 'self'"
    "style-src 'self'"
    "img-src 'self'"
    "form-action 'self'"
    "base-uri 'none'"
    "frame-ancestors 'none'"
  ];

  # Inherited by every vhost, as long as none sets add_header of its own.
  securityHeaders = {
    Strict-Transport-Security = "max-age=31536000; includeSubDomains";
    X-Content-Type-Options = "nosniff";
    # Leaks no URLs to other sites, yet keeps Origin on our own forms' POSTs,
    # which no-referrer would send as "null".
    Referrer-Policy = "same-origin";
    Content-Security-Policy = contentSecurityPolicy;
  };

  # Invite codes are secrets: a request for /invite/<code>, or from a page
  # under it, is logged without the code.
  withoutInviteCodes = variable: logged: ''
    map ${variable} ${logged} {
      "~^/invite/" "/invite/[redacted]";
      "~*invite" "[redacted]";
      default ${variable};
    }
  '';
in
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
    commonHttpConfig = withoutInviteCodes "$request_uri" "$logged_uri"
      + withoutInviteCodes "$http_referer" "$logged_referer" + ''
      log_format anonymous '[$time_local] $host "$request_method $logged_uri $server_protocol" '
                           '$status $body_bytes_sent "$logged_referer" "$http_user_agent"';
      access_log /var/log/nginx/access.log anonymous;
    '' + lib.concatStrings (lib.mapAttrsToList
      (name: value: "add_header ${name} \"${value}\" always;\n")
      securityHeaders);
    # Errors about a request carry "client: <ip>" at any level; emerg is
    # left with little but failures to start.
    logError = "stderr emerg";
  };
}
