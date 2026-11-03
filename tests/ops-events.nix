# ops-event and ops-notify against stand-ins for systemctl and louro,
# which answer from files under $OPS_ROOT/stub and log what they're asked.
# A property's file of several lines answers each read with the next,
# then keeps its last.
{ pkgs, ... }:

let
  systemctl = pkgs.writeShellScriptBin "systemctl" ''
    stub=$OPS_ROOT/stub
    case "$*" in
      is-system-running) cat "$stub/system-state" ;;
      "is-system-running --wait") ;;
      "-q is-active "*) [ -e "$stub/active/$3" ] ;;
      "list-jobs --no-legend") cat "$stub/jobs" 2>/dev/null || true ;;
      "show -P "*)
        property=$stub/$4.$3
        head -n 1 "$property"
        if [ "$(wc -l <"$property")" -gt 1 ]; then sed -i 1d "$property"; fi
        ;;
      *) echo "systemctl stand-in: $*" >&2; exit 1 ;;
    esac
  '';

  louro = pkgs.writeShellScriptBin "louro" ''
    stub=$OPS_ROOT/stub
    case $1 in
      status) [ -e "$stub/louro-status" ] && cat "$stub/louro-status" || exit 3 ;;
      send) echo "$4" >>"$stub/sent"; exit "$(cat "$stub/send-status" 2>/dev/null || echo 0)" ;;
    esac
  '';

  opsEvents = pkgs.callPackage ../pkgs/ops-events {
    systemd = systemctl;
    inherit louro;
    host = "credenza";
    socket = "/run/louro/louro.sock";
  };
in
pkgs.runCommand "ops-events"
{
  nativeBuildInputs = [ opsEvents ];
} ''
  bash ${./ops-events.sh}
  touch $out
''
