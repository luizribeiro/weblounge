{ config, lib, pkgs, ... }:

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

  # A directive set to null is left out, and one set to "" stands alone.
  renderPolicy = policy: lib.concatStringsSep "; " (lib.mapAttrsToList
    (directive: sources: lib.concatStringsSep " " ([ directive ] ++ lib.optional (sources != "") sources))
    (lib.filterAttrs (_: sources: sources != null) policy));

  securityHeaders = policy: {
    Strict-Transport-Security = "max-age=31536000; includeSubDomains";
    X-Content-Type-Options = "nosniff";
    # Leaks no URLs to other sites, yet keeps Origin on our own forms' POSTs,
    # which no-referrer would send as "null".
    Referrer-Policy = "same-origin";
    Content-Security-Policy = renderPolicy (defaultPolicy // policy);
  };

  addHeaders = headers: lib.concatStrings (lib.mapAttrsToList
    (name: value: "add_header ${name} \"${value}\" always;\n")
    headers);

  # The not-found page links back to the landing page, whose origin the
  # web build doesn't know.
  errorPages = pkgs.runCommand "weblounge-error-pages" { } ''
    cp -r --no-preserve=mode ${pkgs.weblounge.web}/errors $out
    substituteInPlace $out/_error/404.html \
      --replace-fail @landing@ ${config.weblounge.origin config.weblounge.domain}/
  '';

  # The pages are only for nginx to answer errors with; what they load is
  # public. Neither is proxied, so they work while an upstream is down.
  serveErrorPages = pages: {
    extraConfig = lib.concatStrings (lib.mapAttrsToList
      (codes: page: "error_page ${codes} /_error/${page};\n")
      pages);
    locations."^~ /_error/" = {
      root = errorPages;
      extraConfig = "internal;";
    };
    locations."^~ /_error/assets/" = {
      root = errorPages;
      extraConfig = "expires 1h;";
    };
  };
in
{
  # Every vhost inherits the security headers, as long as it sets no
  # add_header of its own; one that does takes all of them from here.
  options.weblounge.nginx.securityHeaders = lib.mkOption {
    type = lib.types.functionTo lib.types.lines;
    readOnly = true;
    default = policy: addHeaders (securityHeaders policy);
    description = "add_header lines for the security headers, with these CSP directives over the default policy.";
  };

  options.weblounge.nginx.securityHeadersWith = lib.mkOption {
    type = lib.types.functionTo (lib.types.functionTo lib.types.lines);
    readOnly = true;
    default = policy: headers: addHeaders (securityHeaders policy // headers);
    description = "add_header lines for the security headers, with these CSP directives over the default policy, and these headers over theirs.";
  };

  options.weblounge.nginx.errorPages = lib.mkOption {
    type = lib.types.functionTo lib.types.attrs;
    readOnly = true;
    default = serveErrorPages;
    description = "A vhost's settings to answer errors with our pages, given as { \"<codes>\" = \"<page>.html\"; }.";
  };

  config = {
    networking.firewall.allowedTCPPorts = [ 80 443 ];

    weblounge.deployChecks = lib.mapAttrs'
      (name: _: lib.nameValuePair "https://${name} answers" {
        runs = "outside";
        script = ''
          status=$(${config.weblounge.outsideCurl} -o /dev/null -w '%{http_code}' ${config.weblounge.origin name}/) || exit 1
          [ "$status" -lt 500 ] || { echo "it answered $status"; exit 1; }
        '';
      })
      config.services.nginx.virtualHosts;

    security.acme.acceptTerms = true;

    # An order fails while the CA can't see the challenge yet, as when a new
    # name hasn't reached every nameserver, and nixpkgs leaves it to the
    # daily renewal timer. Retrying after nixpkgs' RestartSec of 15 minutes,
    # at most four times in two hours, stays within Let's Encrypt's five
    # failed validations per name an hour.
    systemd.services = lib.mapAttrs'
      (cert: _: lib.nameValuePair "acme-order-renew-${cert}" {
        serviceConfig.Restart = "on-failure";
        startLimitBurst = 4;
        startLimitIntervalSec = 2 * 60 * 60;
      })
      config.security.acme.certs;

    services.nginx = {
      enable = true;
      recommendedGzipSettings = true;
      recommendedOptimisation = true;
      recommendedProxySettings = true;
      recommendedTlsSettings = true;
      # We promise members that no IP addresses are kept anywhere. $uri
      # becomes an error page's once nginx answers with one, so the log
      # takes the path asked for from the request line, without its query.
      commonHttpConfig = ''
        map $request_uri $request_path {
          ~^([^?]*) $1;
        }
        log_format anonymous '[$time_local] $host "$request_method $request_path $server_protocol" '
                             '$status $body_bytes_sent "$http_referer" "$http_user_agent"';
        access_log /var/log/nginx/access.log anonymous;
      '' + config.weblounge.nginx.securityHeaders { };
      # Errors about a request carry "client: <ip>" at any level; emerg is
      # left with little but failures to start.
      logError = "stderr emerg";
    };
  };
}
