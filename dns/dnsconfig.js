var zones = require("./zones.json");
var builders = { A: A, CNAME: CNAME, MX: MX, TXT: TXT };

zones.forEach(function (zone) {
  D(
    zone.domain,
    NewRegistrar("none"),
    DnsProvider(NewDnsProvider("desec")),
    DefaultTTL(zone.ttl),
    NAMESERVER_TTL(zone.ttl),
    zone.records.map(function (record) {
      return builders[record[0]].apply(null, record.slice(1));
    }),
  );
});
