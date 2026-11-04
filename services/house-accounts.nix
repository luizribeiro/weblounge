# House accounts: Ergo accounts of the network's own, neither members nor
# members' bots. Each gets a generated password in RAM and a
# <name>-account oneshot, which holds Ergo's API token, registers the
# account or puts the password back, and gives it its vhost and
# always-on; whatever connects as it starts after it, with the password.
{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.ergo.houseAccounts;
  inherit (config.weblounge) sandbox loopbackOnly;
  inherit (config.weblounge.concierge) database;

  # Ergo's default accounts.vhosts.valid-regexp and max-length, and
  # concierge's rule for nicks; crates/concierge checks the same.
  validVhost = vhost: builtins.match "[0-9A-Za-z./_-]{1,64}" vhost != null;
  validName = name: builtins.match "[A-Za-z][A-Za-z0-9_-]{0,31}" name != null;

  houseAccount = { name, ... }: {
    options = {
      vhost = lib.mkOption {
        type = lib.types.str;
        example = "weblounge/${name}";
        description = "The host everyone sees the account connect from.";
      };
      unit = lib.mkOption {
        type = lib.types.str;
        readOnly = true;
        default = "${name}-account";
        description = "The oneshot that sets the account up, which whatever connects as it requires.";
      };
      password = lib.mkOption {
        type = lib.types.str;
        readOnly = true;
        default = "${name}-password";
        description = "The secret holding the account's password; add what connects as it to its loadedBy.";
      };
    };
  };

  ensure = name: account: lib.mkMerge [
    database.unit
    {
      description = "Register the house account ${name}, or set its password";
      wants = [ "ergochat.service" ];
      after = [ "ergochat.service" ];
      environment.CONCIERGE_ERGO_API = "http://${config.weblounge.ergo.settings.api.listener}";
      script = config.weblounge.ergo.untilItAnswers
        "${lib.getExe pkgs.weblounge.concierge} ensure-house-account ${lib.escapeShellArgs [ name account.vhost ]}";
      serviceConfig = sandbox // loopbackOnly // {
        Type = "oneshot";
        RemainAfterExit = true;
      };
    }
  ];
in
{
  # concierge.nix, whose database the oneshots open, imports this.
  imports = [ ./ergo.nix ./sandbox.nix ./secrets.nix ];

  options.weblounge.ergo.houseAccounts = lib.mkOption {
    type = lib.types.attrsOf (lib.types.submodule houseAccount);
    default = { };
    description = "The network's own accounts, by name, which nobody may sign up or make a bot under.";
  };

  options.weblounge.ergo.houseAccountNames = lib.mkOption {
    type = lib.types.str;
    readOnly = true;
    default = lib.concatStringsSep "," (lib.attrNames cfg);
    description = "The house accounts' names, separated by commas, as concierge's CONCIERGE_HOUSE_ACCOUNTS takes them.";
  };

  config = {
    assertions = lib.concatLists (lib.mapAttrsToList
      (name: account: [
        {
          assertion = validName name;
          message = "House account ${name}'s name must follow the rule for nicks: a letter, then up to 31 letters, digits, hyphens and underscores.";
        }
        {
          assertion = validVhost account.vhost;
          message = "House account ${name}'s vhost ${account.vhost} must be 1 to 64 letters, digits and . - _ /, as Ergo accepts.";
        }
      ])
      cfg);

    weblounge.secrets = lib.mkMerge (lib.mapAttrsToList
      (_: account: {
        ${account.password} = {
          ephemeral = true;
          loadedBy = [ account.unit ];
        };
        ergo-api-token.loadedBy = [ account.unit ];
      })
      cfg);

    systemd.services = lib.mapAttrs' (name: account: lib.nameValuePair account.unit (ensure name account)) cfg;
  };
}
