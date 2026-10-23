# ops-notify posts a line to #ops through louro.
{ writeShellApplication
, coreutils
, jq
, louro
, socket
}:

writeShellApplication {
  name = "ops-notify";
  runtimeInputs = [ coreutils jq louro ];
  runtimeEnv.LOURO_SOCKET = socket;
  text = builtins.readFile ./ops-notify.sh;
}
