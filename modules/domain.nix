{ config, lib, ... }:

let
  inherit (config.weblounge) httpsPort;
in
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
    selfSignedCa = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "The CA behind this machine's certificates when no public CA issued them, for its own services to trust.";
    };
    origin = lib.mkOption {
      type = lib.types.functionTo lib.types.str;
      readOnly = true;
      default = host: "https://${host}" + lib.optionalString (httpsPort != 443) ":${toString httpsPort}";
      description = "The origin browsers see for a host name, with the HTTPS port when it isn't 443.";
    };
  };

  config.networking.domain = config.weblounge.domain;
}
