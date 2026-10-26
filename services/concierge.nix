{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.concierge;
  hostName = "members.${config.weblounge.domain}";
  publicUrl = config.weblounge.origin hostName;
  joinHostName = "join.${config.weblounge.domain}";
  concierge = lib.getExe pkgs.weblounge.concierge;
  multiclient = config.weblounge.ergo.settings.accounts.multiclient;
  historyExpireTime = config.weblounge.ergo.settings.history.restrictions.expire-time;

  # concierge-admin runs with the rest of the unit's serviceConfig, so admin
  # commands see exactly the identity, state and secrets the service does.
  serviceOnly = {
    ExecStart = "${concierge} serve";
    Restart = "on-failure";
  };

  sharedProperties = removeAttrs config.systemd.services.concierge.serviceConfig
    (lib.attrNames serviceOnly);

  # Every unit that opens concierge's database runs as concierge, with its
  # state.
  stateAccess = {
    User = "concierge";
    DynamicUser = true;
    StateDirectory = "concierge";
    StateDirectoryMode = "0700";
  };

  # The outbox is the one database concierge shares with its mailer, which
  # runs as a user of its own; they share a group, and the outbox's
  # directory alone.
  outboxGroup = "concierge-outbox";
  outboxDir = "/var/lib/concierge-outbox";
  outboxPath = "${outboxDir}/outbox.db";
  outboxAccess = {
    SupplementaryGroups = [ outboxGroup ];
    # concierge-migrate creates it, so it may be missing as that starts.
    ReadWritePaths = [ "-${outboxDir}" ];
  };

  # Concierge hands uploads to the scrubber, and deletes the files it
  # stored; it never reads them.
  inherit (config.weblounge) files;
  filesEnvironment = {
    CONCIERGE_UPLOADS = files.directory;
    CONCIERGE_SCRUBBER = files.scrubber.socket;
    CONCIERGE_FILES_URL = config.weblounge.origin files.hostName;
  };
  filesAccess = {
    SupplementaryGroups = [ files.group ];
    ReadWritePaths = [ files.directory ];
  };
  scrubberAccess.SupplementaryGroups = [ files.scrubber.group ];
  withAccess = accesses: lib.zipAttrsWith (_: lib.concatLists) accesses;

  # SQLite doesn't keep two processes from migrating at once, so one unit
  # migrates before any other opens either database.
  migrated = {
    requires = [ "concierge-migrate.service" ];
    after = [ "concierge-migrate.service" ];
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
  imports = [ ./ergo.nix ./files.nix ./loopback.nix ./nginx.nix ./sandbox.nix ./secrets.nix ./sqlite-vacuum.nix ];

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

  options.weblounge.concierge.outbox = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    readOnly = true;
    default = {
      path = outboxPath;
      directory = outboxDir;
      unit = migrated // { serviceConfig = outboxAccess; };
    };
    description = "The outbox's database, and the settings of a unit that opens it once it's migrated.";
  };

  options.weblounge.concierge.database = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    readOnly = true;
    default.unit = migrated // { serviceConfig = stateAccess; };
    description = "The settings of a unit that opens concierge's database once it's migrated, as concierge.";
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
    weblounge.secrets.oauth-introspection-secret = {
      ephemeral = true;
      loadedBy = [ "concierge" ];
    };

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

    users.groups.${outboxGroup} = { };

    systemd.services.concierge-migrate = {
      description = "Bring concierge's database and outbox up to date";
      environment.CONCIERGE_OUTBOX = outboxPath;
      serviceConfig = config.weblounge.sandbox // stateAccess // outboxAccess // {
        Type = "oneshot";
        RemainAfterExit = true;
        # SQLite creates databases readable by their owner alone, but gives
        # the -wal and -shm files the database's mode, so an empty outbox
        # that its group may write lets concierge and the mailer both in.
        # Files in the directory take its group.
        ExecStartPre = "+" + pkgs.writeShellScript "concierge-outbox" ''
          ${pkgs.coreutils}/bin/install -d -m 2770 -g ${outboxGroup} ${outboxDir}
          [ -e ${outboxPath} ] || ${pkgs.coreutils}/bin/install -m 0660 -g ${outboxGroup} /dev/null ${outboxPath}
        '';
        ExecStart = "${concierge} migrate";
        PrivateNetwork = true;
        RestrictAddressFamilies = "none";
      };
    };

    systemd.services.concierge = lib.mkMerge [
      migrated
      {
        description = "Concierge, the members' site";
        wantedBy = [ "multi-user.target" ];
        after = [ "network.target" ];
        serviceConfig = config.weblounge.sandbox // config.weblounge.loopbackOnly // stateAccess // serviceOnly
          // withAccess [ outboxAccess filesAccess scrubberAccess ] // {
          RestrictAddressFamilies = config.weblounge.sandbox.RestrictAddressFamilies ++ [ "AF_UNIX" ];
          Environment = lib.mapAttrsToList (name: value: "${name}=${value}") filesEnvironment ++ [
            "CONCIERGE_OUTBOX=${outboxPath}"
            "CONCIERGE_LISTEN=${cfg.listen}"
            "CONCIERGE_PUBLIC_URL=${publicUrl}"
            "CONCIERGE_JOIN_URL=${config.weblounge.origin joinHostName}"
            "CONCIERGE_WEBCHAT_URL=${config.weblounge.origin config.weblounge.ergo.serverName}"
            "CONCIERGE_IRC_SERVER=${config.weblounge.ergo.address}"
            "CONCIERGE_ERGO_API=http://${config.weblounge.ergo.settings.api.listener}"
            "CONCIERGE_ASSETS=${pkgs.weblounge.web}/members"
            "CONCIERGE_ALWAYS_ON_DEFAULT=${multiclient.always-on}"
            "CONCIERGE_AUTO_AWAY_DEFAULT=${multiclient.auto-away}"
            "CONCIERGE_HISTORY_EXPIRE_TIME=${historyExpireTime}"
          ];
        };
      }
    ];

    # The one reminder to confirm an email, due from the fifth day. It
    # touches nothing but the two databases, so it gets no network and no
    # credentials.
    systemd.services.concierge-remind = lib.mkMerge [
      migrated
      {
        description = "Remind members to confirm their email";
        startAt = "hourly";
        environment = {
          CONCIERGE_OUTBOX = outboxPath;
          CONCIERGE_PUBLIC_URL = publicUrl;
        };
        serviceConfig = config.weblounge.sandbox // stateAccess // outboxAccess // {
          Type = "oneshot";
          ExecStart = "${concierge} remind";
          PrivateNetwork = true;
          RestrictAddressFamilies = "none";
        };
      }
    ];
    # A run missed while the machine was down happens once it's back.
    systemd.timers.concierge-remind.timerConfig.Persistent = true;

    # Uploads stay 30 days. It touches nothing but concierge's database
    # and the uploads directory.
    systemd.services.concierge-expire-uploads = lib.mkMerge [
      migrated
      {
        description = "Delete the uploads past their days";
        startAt = "hourly";
        environment = filesEnvironment;
        serviceConfig = config.weblounge.sandbox // stateAccess // filesAccess // {
          Type = "oneshot";
          ExecStart = "${concierge} expire-uploads";
          PrivateNetwork = true;
          RestrictAddressFamilies = "none";
        };
      }
    ];
    systemd.timers.concierge-expire-uploads.timerConfig.Persistent = true;

    environment.systemPackages = [ admin ];

    services.nginx.virtualHosts.${hostName} = lib.mkMerge [
      # While concierge is down or stuck. The page keeps nginx's status,
      # which tells a 502's refused connection from a 504's timeout to
      # deploy checks and logs. Concierge's own errors pass through, and it
      # answers its own 404s, so the bare one below stays bare.
      (config.weblounge.nginx.errorPages { "502 503 504" = "down.html"; })
      {
        enableACME = true;
        forceSSL = true;
        # Signup and password forms fetch the password rules and nick
        # availability as members type, and the Uploads tab shows each upload
        # from where it's served.
        extraConfig = config.weblounge.nginx.securityHeaders {
          connect-src = "'self'";
          img-src = "'self' ${filesEnvironment.CONCIERGE_FILES_URL}";
        };
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
        # Only Ergo introspects tokens, on loopback, and uploads come in on
        # files.weblounge.org alone.
        locations."= /oauth/introspect".return = "404";
        locations."= /upload".return = "404";
      }
    ];

    # Invite links' short home. Browsers carry the code in the fragment
    # across the redirect, and never send it here.
    services.nginx.virtualHosts.${joinHostName} = lib.mkMerge [
      (config.weblounge.nginx.errorPages { "404" = "404.html"; })
      {
        enableACME = true;
        forceSSL = true;
        locations."= /".return = "302 ${publicUrl}/invite";
        locations."/".return = "404";
      }
    ];
  };
}
