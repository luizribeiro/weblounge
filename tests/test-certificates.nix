# The VM tests' certificates verify against their CA, and a second build,
# made under another name so that it really runs, makes the same files.
{ pkgs, ... }:

let
  certificates = import ./e2e/lib/certificates.nix { inherit pkgs; } "mail";
  rebuilt = certificates.overrideAttrs { name = "mail-test-certificates-rebuilt"; };
in
pkgs.runCommand "test-certificates" { nativeBuildInputs = [ pkgs.openssl ]; } ''
  openssl verify -x509_strict -verify_hostname mail -CAfile ${certificates}/ca.pem ${certificates}/mail/cert.pem
  diff -r ${certificates} ${rebuilt}
  touch $out
''
