# Ergo's config: the given settings merged over the default.yaml it ships.
# default.yaml's listeners, WebIRC gateway and admin oper are live
# examples, with published credentials, rather than defaults. Resetting the
# root's style keeps empty settings, written as a flow-style {}, from turning
# the whole config into flow style, which Ergo can't load.
{ runCommand, formats, yq-go, openssl, ergochat }:

settings: runCommand "ergo.yaml" { nativeBuildInputs = [ yq-go openssl ]; } ''
  yq eval-all '
    (select(fi == 0) | del(.server.listeners, .server.webirc, .opers))
      * select(fi == 1)
    | ... comments = ""
    | . style = ""
  ' ${ergochat.src}/default.yaml ${(formats.yaml { }).generate "ergo-settings.yaml" settings} > $out

  # Ergo loads the config as it would on the server, so one it would refuse
  # fails here. The real certificate only exists there, a config needs a
  # listener to load, and nixpkgs' Ergo ships no translations.
  mkdir check && cd check
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj /CN=check \
    -keyout key.pem -out cert.pem 2>/dev/null
  ln -s ${ergochat.src}/languages languages
  yq '
    .server.listeners = (.server.listeners // {"127.0.0.1:6667": {}})
    | (.server.listeners[] | select(has("tls")) | .tls) = {"cert": "cert.pem", "key": "key.pem"}
  ' $out > ircd.yaml
  ${ergochat}/bin/ergo run --conf ircd.yaml --smoke --quiet
''
