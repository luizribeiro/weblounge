{ config, lib, ... }:

{
  options.weblounge.domain = lib.mkOption {
    type = lib.types.str;
    default = "weblounge.org";
    description = "The domain every public name, certificate and link derives from.";
  };

  config.networking.domain = config.weblounge.domain;
}
