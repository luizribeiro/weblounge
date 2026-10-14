{ config, lib, ... }:

let
  defaultPolicy = {
    default-src = "'none'";
    script-src = "'self'";
    style-src = "'self'";
    img-src = "'self'";
    form-action = "'self'";
    base-uri = "'none'";
    frame-ancestors = "'none'";
  };

  renderPolicy = policy: lib.concatStringsSep "; "
    (lib.mapAttrsToList (directive: sources: "${directive} ${sources}") policy);

  securityHeaders = policy: {
    Strict-Transport-Security = "max-age=31536000; includeSubDomains";
    X-Content-Type-Options = "nosniff";
    # Leaks no URLs to other sites, yet keeps Origin on our own forms' POSTs,
    # which no-referrer would send as "null".
    Referrer-Policy = "same-origin";
    Content-Security-Policy = renderPolicy (defaultPolicy // policy);
  };

  addHeaders = policy: lib.concatStrings (lib.mapAttrsToList
    (name: value: "add_header ${name} \"${value}\" always;\n")
    (securityHeaders policy));

  # Invite codes are secrets. Paths are logged decoded and without their
  # query, so no spelling of /invite/<code> gets through; referers, which
  # nginx doesn't decode, are left out whenever they could hold a code.
  inviteRedaction = ''
    map $uri $logged_uri {
      "~*^/invite/" "/invite/[redacted]";
      default $uri;
    }
    map $http_referer $logged_referer {
      "~*invite|%" "[redacted]";
      default $http_referer;
    }
  '';
in
{
  # Every vhost inherits the security headers, as long as it sets no
  # add_header of its own; one that does takes all of them from here.
  options.weblounge.nginx.securityHeaders = lib.mkOption {
    type = lib.types.functionTo lib.types.lines;
    readOnly = true;
    default = addHeaders;
    description = "add_header lines for the security headers, with these CSP directives over the default policy.";
  };

  config = {
    networking.firewall.allowedTCPPorts = [ 80 443 ];

    security.acme.acceptTerms = true;

    services.nginx = {
      enable = true;
      recommendedGzipSettings = true;
      recommendedOptimisation = true;
      recommendedProxySettings = true;
      recommendedTlsSettings = true;
      # We promise members that no IP addresses are kept anywhere.
      commonHttpConfig = inviteRedaction + ''
        log_format anonymous '[$time_local] $host "$request_method $logged_uri $server_protocol" '
                             '$status $body_bytes_sent "$logged_referer" "$http_user_agent"';
        access_log /var/log/nginx/access.log anonymous;
      '' + config.weblounge.nginx.securityHeaders { };
      # Errors about a request carry "client: <ip>" at any level; emerg is
      # left with little but failures to start.
      logError = "stderr emerg";
    };
  };
}
