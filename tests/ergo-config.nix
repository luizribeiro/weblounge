{ pkgs, weblounge, ... }:

let
  overlaid = pkgs.extend weblounge.overlay;
  refused = pkgs.testers.testBuildFailure (overlaid.weblounge.ergoConfig { server.max-sendq = "lots"; });
in
pkgs.runCommand "ergo-config" { } ''
  grep -q "Could not parse maximum SendQ size" ${refused}/testBuildFailure.log
  touch $out
''
