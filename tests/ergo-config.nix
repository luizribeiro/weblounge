{ pkgs, weblounge, ... }:

let
  overlaid = pkgs.extend weblounge.overlay;
in
pkgs.runCommand "ergo-config" { } ''
  # The base config has no listeners, and nixpkgs' Ergo ships no translations.
  ln -s ${overlaid.ergochat.src}/languages languages
  ERGO__SERVER__LISTENERS="{\"$PWD/irc.sock\": {}}" \
    ${overlaid.ergochat}/bin/ergo run --conf ${overlaid.weblounge.ergoConfig { }} --smoke --quiet
  touch $out
''
