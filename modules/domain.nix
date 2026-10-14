{ config, lib, ... }:

{
  options.weblounge = {
    domain = lib.mkOption {
      type = lib.types.str;
      default = "weblounge.org";
      description = "The domain every public name, certificate and link derives from.";
    };
    httpsPort = lib.mkOption {
      type = lib.types.port;
      default = 443;
      description = "The port browsers reach HTTPS on, which links must carry when it isn't 443.";
    };
    ircPort = lib.mkOption {
      type = lib.types.port;
      default = 6697;
      description = "The port IRC clients reach Ergo's TLS listener on, as members are told to connect.";
    };
  };

  config.networking.domain = config.weblounge.domain;
}
