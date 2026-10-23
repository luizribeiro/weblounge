# ops-notify posts a line to #ops through louro; ops-event turns the
# host's events into such lines. services/ops-events.nix wires them up.
{ symlinkJoin
, writeShellApplication
, coreutils
, gawk
, gnugrep
, gnused
, jq
, systemd
, louro
, host
, socket
}:

let
  ops-notify = writeShellApplication {
    name = "ops-notify";
    runtimeInputs = [ coreutils jq louro ];
    runtimeEnv.LOURO_SOCKET = socket;
    text = builtins.readFile ./ops-notify.sh;
  };

  ops-event = writeShellApplication {
    name = "ops-event";
    runtimeInputs = [ coreutils gawk gnugrep gnused systemd ops-notify ];
    runtimeEnv.OPS_HOST = host;
    text = builtins.readFile ./ops-event.sh;
  };
in
symlinkJoin {
  name = "ops-events";
  paths = [ ops-notify ops-event ];
  meta.mainProgram = "ops-event";
  passthru = { inherit ops-notify ops-event; };
}
