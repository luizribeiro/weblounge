# A test CA and a certificate from it for domain: ca.pem is the CA's, and
# <domain>/cert.pem and <domain>/key.pem the server's.
#
# The build must be reproducible: CI jobs substitute it from a cache, and
# one that trusts a CA from one build while serving a certificate from
# another fails. Keys come from fixed seeds and certificates have fixed
# dates and serials, so every build makes the same bytes.
{ pkgs }:

domain:
let
  seed = name: builtins.hashString "sha256" "weblounge test key for ${name}";
  template = name: lines: pkgs.writeText "${name}.cfg" (pkgs.lib.concatLines ([
    ''activation_date = "2020-01-01 00:00:00 UTC"''
    ''expiration_date = "2099-12-31 23:59:59 UTC"''
  ] ++ lines));
  ca = template "ca" [ ''cn = "weblounge test CA for ${domain}"'' "serial = 1" "ca" "cert_signing_key" ];
  server = template domain [
    ''cn = "${domain}"''
    ''dns_name = "${domain}"''
    "serial = 2"
    "tls_www_server"
    "signing_key"
    "encryption_key"
  ];
in
pkgs.runCommand "${domain}-test-certificates" { nativeBuildInputs = [ pkgs.gnutls ]; } ''
  key() { certtool --generate-privkey --rsa --bits 2048 --provable --seed "$1" --outfile "$2"; }
  mkdir -p $out/${domain} && cd $out
  key ${seed "the CA of ${domain}"} ca-key.pem
  certtool --generate-self-signed --load-privkey ca-key.pem --template ${ca} --outfile ca.pem
  key ${seed domain} ${domain}/key.pem
  certtool --generate-certificate --load-privkey ${domain}/key.pem \
    --load-ca-certificate ca.pem --load-ca-privkey ca-key.pem \
    --template ${server} --outfile ${domain}/cert.pem
''
