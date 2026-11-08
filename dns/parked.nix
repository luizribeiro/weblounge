# A domain we hold but don't serve yet. It sends and takes no mail, and
# says so, so nobody can pass off mail as coming from it.
{ domain, ttl }:

{
  inherit domain ttl;
  records = [
    [ "MX" "@" 0 "." ]
    [ "TXT" "@" "v=spf1 -all" ]
    [ "TXT" "_dmarc" "v=DMARC1; p=reject" ]
  ];
}
