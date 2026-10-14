# Ergo's config: the given settings merged over the default.yaml it ships.
# default.yaml's listeners, WebIRC gateway and admin oper are live
# examples, with published credentials, rather than defaults.
{ runCommand, formats, yq-go, ergochat }:

settings: runCommand "ergo.yaml" { nativeBuildInputs = [ yq-go ]; } ''
  yq eval-all '
    (select(fi == 0) | del(.server.listeners, .server.webirc, .opers))
      * select(fi == 1)
    | ... comments = ""
  ' ${ergochat.src}/default.yaml ${(formats.yaml { }).generate "ergo-settings.yaml" settings} > $out
''
