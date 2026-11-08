{ runCommand, writeText, dnscontrol, zones }:

runCommand "dnsconfig" { nativeBuildInputs = [ dnscontrol ]; } ''
  mkdir $out
  cp ${./dnsconfig.js} $out/dnsconfig.js
  cp ${./creds.json} $out/creds.json
  cp ${writeText "zones.json" (builtins.toJSON zones)} $out/zones.json
  dnscontrol check --config $out/dnsconfig.js
''
