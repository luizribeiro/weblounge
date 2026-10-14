{ runCommand, writeText, dnscontrol, zone }:

runCommand "dnsconfig" { nativeBuildInputs = [ dnscontrol ]; } ''
  mkdir $out
  cp ${./dnsconfig.js} $out/dnsconfig.js
  cp ${./creds.json} $out/creds.json
  cp ${writeText "zone.json" (builtins.toJSON zone)} $out/zone.json
  dnscontrol check --config $out/dnsconfig.js
''
