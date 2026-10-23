# ops-notify against a stand-in for louro, which answers from files
# under $OPS_ROOT/stub and logs what it's asked.
{ pkgs, ... }:

let
  louro = pkgs.writeShellScriptBin "louro" ''
    stub=$OPS_ROOT/stub
    case $1 in
      status) [ -e "$stub/louro-status" ] && cat "$stub/louro-status" || exit 3 ;;
      send) echo "$4" >>"$stub/sent"; exit "$(cat "$stub/send-status" 2>/dev/null || echo 0)" ;;
    esac
  '';

  opsNotify = pkgs.callPackage ../pkgs/ops-events {
    inherit louro;
    socket = "/run/louro/louro.sock";
  };
in
pkgs.runCommand "ops-events"
{
  nativeBuildInputs = [ opsNotify ];
} ''
  bash ${./ops-events.sh}
  touch $out
''
