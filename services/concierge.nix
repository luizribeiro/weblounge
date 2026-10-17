{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.concierge;
  hostName = "members.${config.weblounge.domain}";
  publicUrl = config.weblounge.origin hostName;
  joinHostName = "join.${config.weblounge.domain}";
  concierge = lib.getExe pkgs.weblounge.concierge;
  multiclient = config.weblounge.ergo.settings.accounts.multiclient;

  # concierge-admin runs with the rest of the unit's serviceConfig, so admin
  # commands see exactly the identity, state and secrets the service does.
  serviceOnly = {
    ExecStart = "${concierge} serve";
    Restart = "on-failure";
  };

  sharedProperties = removeAttrs config.systemd.services.concierge.serviceConfig
    (lib.attrNames serviceOnly);

  # Every unit that opens the database runs as concierge, with its state.
  stateAccess = {
    User = "concierge";
    DynamicUser = true;
    StateDirectory = "concierge";
    StateDirectoryMode = "0700";
  };

  # SQLite doesn't keep two processes from migrating at once, so one unit
  # migrates before any other opens the database.
  databaseUnit = {
    requires = [ "concierge-migrate.service" ];
    after = [ "concierge-migrate.service" ];
    serviceConfig = stateAccess;
  };

  admin = pkgs.writeShellApplication {
    name = "concierge-admin";
    text = ''
      exec ${config.security.wrapperDir}/sudo ${config.systemd.package}/bin/systemd-run \
        --quiet --pipe --wait --collect --service-type=exec \
        ${lib.escapeShellArgs (config.weblounge.runProperties sharedProperties)} \
        ${concierge} "$@"
    '';
  };
in
{
  imports = [ ./ergo.nix ./loopback.nix ./nginx.nix ./sandbox.nix ./secrets.nix ./sqlite-vacuum.nix ];

  options.weblounge.concierge.publicUrl = lib.mkOption {
    type = lib.types.str;
    readOnly = true;
    default = publicUrl;
    description = "The members' site's origin, which is also the OAuth issuer.";
  };

  options.weblounge.concierge.admin = lib.mkOption {
    type = lib.types.package;
    readOnly = true;
    default = admin;
    description = "concierge-admin, which runs an admin command as the service.";
  };

  options.weblounge.concierge.databaseUnit = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    readOnly = true;
    default = databaseUnit;
    description = "Settings for a unit that opens concierge's database: its identity and state, once migrated.";
  };

  options.weblounge.concierge.listen = lib.mkOption {
    type = lib.types.str;
    default = "127.0.0.1:8080";
    description = "Where concierge listens for nginx, as host:port.";
  };

  config = {
    weblounge.loopbackListeners.Concierge = cfg.listen;

    # The account home's switches only make sense where members can choose.
    assertions = lib.mapAttrsToList
      (name: setting: {
        assertion = lib.elem setting [ "opt-in" "opt-out" ];
        message = "Concierge lets members switch Ergo's ${name}, so it must be opt-in or opt-out, not ${toString setting}.";
      })
      { inherit (multiclient) always-on auto-away; };

    weblounge.secrets.session-key.loadedBy = [ "concierge" ];
    weblounge.secrets.ergo-api-token.loadedBy = [ "concierge" ];
    weblounge.secrets.oauth-introspection-secret.loadedBy = [ "concierge" ];

    # Ergo logs the web client in with SASL OAUTHBEARER, asking concierge
    # whose token it is. The secret comes from the environment; the empty
    # one here replaces default.yaml's published example.
    weblounge.ergo.settings.accounts.oauth2 = {
      enabled = true;
      autocreate = false;
      introspection-url = "http://${cfg.listen}/oauth/introspect";
      client-id = "ergo";
      client-secret = "";
    };

    weblounge.sqliteVacuum.concierge = "/var/lib/concierge/concierge.db";

    systemd.services.concierge-migrate = {
      description = "Bring concierge's database up to date";
      serviceConfig = config.weblounge.sandbox // stateAccess // {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${concierge} migrate";
        PrivateNetwork = true;
        RestrictAddressFamilies = "none";
      };
    };

    systemd.services.concierge = lib.mkMerge [
      databaseUnit
      {
        description = "Concierge, the members' site";
        wantedBy = [ "multi-user.target" ];
        after = [ "network.target" ];
        serviceConfig = config.weblounge.sandbox // config.weblounge.loopbackOnly // serviceOnly // {
          Environment = [
            "CONCIERGE_LISTEN=${cfg.listen}"
            "CONCIERGE_PUBLIC_URL=${publicUrl}"
            "CONCIERGE_JOIN_URL=${config.weblounge.origin joinHostName}"
            "CONCIERGE_WEBCHAT_URL=${config.weblounge.origin config.weblounge.ergo.serverName}"
            "CONCIERGE_IRC_SERVER=${config.weblounge.ergo.address}"
            "CONCIERGE_ERGO_API=http://${config.weblounge.ergo.settings.api.listener}"
            "CONCIERGE_ASSETS=${pkgs.weblounge.web}/members"
            "CONCIERGE_ALWAYS_ON_DEFAULT=${multiclient.always-on}"
            "CONCIERGE_AUTO_AWAY_DEFAULT=${multiclient.auto-away}"
          ];
        };
      }
    ];

    environment.systemPackages = [ admin ];

    services.nginx.virtualHosts.${hostName} = {
      enableACME = true;
      forceSSL = true;
      # Signup and password forms fetch the password rules and nick
      # availability as members type.
      extraConfig = config.weblounge.nginx.securityHeaders { connect-src = "'self'"; };
      locations."/" = {
        proxyPass = "http://${cfg.listen}";
        # Concierge trusts X-Real-IP for its sign-in limits, so nginx sets it
        # from the connection, replacing anything the client sent. The
        # recommended settings would add X-Forwarded-For too, which nothing
        # reads.
        recommendedProxySettings = false;
        extraConfig = ''
          proxy_set_header X-Real-IP $remote_addr;
        '';
      };
      # Only Ergo introspects tokens, on loopback.
      locations."= /oauth/introspect".return = "404";
    };

    # Invite links' short home. Browsers carry the code in the fragment
    # across the redirect, and never send it here.
    services.nginx.virtualHosts.${joinHostName} = {
      enableACME = true;
      forceSSL = true;
      locations."= /".return = "302 ${publicUrl}/invite";
      locations."/".return = "404";
    };
  };
}
