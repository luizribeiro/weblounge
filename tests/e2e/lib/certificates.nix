# A test CA and a certificate from it for domain: minica.pem is the CA's,
# and <domain>/cert.pem and <domain>/key.pem the server's.
{ pkgs }:

domain: pkgs.runCommand "${domain}-test-certificates" { nativeBuildInputs = [ pkgs.minica ]; } ''
  mkdir $out && cd $out && minica --domains ${domain}
''
