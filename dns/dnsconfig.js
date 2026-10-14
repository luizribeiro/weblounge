var zone = require("./zone.json");
var builders = { A: A, CNAME: CNAME, MX: MX, TXT: TXT };

D(
  zone.domain,
  NewRegistrar("none"),
  DnsProvider(NewDnsProvider("desec")),
  DefaultTTL(zone.ttl),
  zone.records.map(function (record) {
    return builders[record[0]].apply(null, record.slice(1));
  }),
);
