# weblounge

[![check](https://github.com/luizribeiro/weblounge/actions/workflows/check.yml/badge.svg)](https://github.com/luizribeiro/weblounge/actions/workflows/check.yml)

Infrastructure for [weblounge.org](https://weblounge.org), an invite-only IRC
network at `irc.weblounge.org`. Everything that runs on the servers lives in
this repository and is deployed from it.

## Getting started

You need [Nix](https://nixos.org/download) with flakes enabled, and ideally
[direnv](https://direnv.net). Then:

```sh
direnv allow       # or: nix develop
just               # list the available commands
```

Entering the dev shell installs the git hooks: formatting and lint run on
every commit, and `nix flake check` (host builds plus VM tests) runs before
every push.

## Working on the website

The landing page's sources live in `web/`: markup in `pages/`, plain CSS in
`styles/`, TypeScript in `scripts/`. Nix builds them with esbuild into
`pkgs.weblounge.web`; there is no npm.

```sh
just web dev       # serve web/ on http://localhost:8000
```

The page reloads whenever a stylesheet or script changes; edits to the HTML
show up on the next manual reload.

## Working on concierge

Concierge, the members' site at `members.weblounge.org`, is a Rust app in
`crates/concierge`: axum, with askama templates in its `templates/`. Its
stylesheet, `web/styles/members.css`, builds on the landing page's tokens
and the components in `web/styles/components/`, and esbuild gives every
asset a hashed name that concierge looks up in the build's manifest. Both
sites draw the wordmark from `web/images/wordmark.svg` with `<use>`, and
concierge's header mark is the favicon's `#mark`. The network chair,
`web/images/chair.svg`, is inlined by both: the landing page's build
replaces `<!-- include images/chair.svg -->` with it, and the Home tab
inlines concierge's built copy, so `scripts/chair` can animate it on
either site.

```sh
just concierge dev                            # run it on http://localhost:8080
just concierge dev-admin invite create --root # an admin command against it
```

bacon restarts concierge whenever its code, a template or a stylesheet
changes, and the open page reloads once it's back. Next to it runs a local
Ergo on the same base config production builds on, its API on
`127.0.0.1:8089` and its log in `target/dev/ergo.log`. Concierge's database,
Ergo's datastore and the dev secrets all live in `target/dev`; delete it to
start over. `cargo test` runs the tests; `just check` adds clippy and
rustfmt.

Concierge keeps its data in SQLite through sqlx, which checks every query
at compile time against the metadata in `.sqlx`. Concierge never sends
mail itself: it queues messages in the outbox, a database of its own
owned by `crates/outbox`, and `crates/mailer`, a separate binary that
depends on neither concierge nor its database, sends them over SMTP as
the `concierge-mailer` unit. `crates/common` holds the little both
share. After changing a query or adding a migration to either database's
`migrations`, run `just concierge prepare` and commit what it writes. On
the server, `concierge-migrate.service` applies new migrations to both
before anything else opens them, so the mailer never migrates; `just
concierge dev` migrates as it starts.

Mail that carries a secret, such as a verification link, goes into the
outbox sealed with [age](https://age-encryption.org) to a key the mailer
generates as it starts, keeps in memory only and withdraws as it stops,
so a copy of the disk never holds the link. Sealed mail still waiting
when the mailer stops is lost, and members ask for another. While no
mailer runs, as in `just concierge dev`, concierge sends no such mail
and says to try again.

## The web client

`https://irc.weblounge.org` serves [gamja](https://codeberg.org/emersion/gamja)
from nixpkgs, patched in `pkgs/gamja.nix`. A member signed in on the
members' site lands in #lounge without typing anything:

1. gamja's `config.json`, generated in `services/webchat.nix`, sends it to
   concierge's `/oauth/authorize`. Concierge, a small OAuth 2.0 server,
   issues a one-minute, single-use code to the signed-in member, or has
   them sign in first.
2. gamja trades the code and its PKCE verifier at `/oauth/token` for an
   access token that lasts as long as the member's session; signing out
   or a password reset ends it.
3. gamja connects to `wss://irc.weblounge.org/socket`, which nginx proxies
   to Ergo's WebSocket listener on `127.0.0.1:8097`, and logs in with SASL
   OAUTHBEARER. Ergo asks concierge's `/oauth/introspect` on loopback whose
   token it is, authenticating with the generated
   `oauth-introspection-secret`, and never creates an account for it.

In the VM it's at https://irc.weblounge.localhost:10443.

`https://beta.weblounge.org` serves the web client that will replace
gamja, [weblounge-client](https://github.com/luizribeiro/weblounge-client),
built by its own flake, the `weblounge-client` input. It signs members in
the same way, as concierge's second OAuth client, `weblounge-client`,
reads its settings from the `/config.json` `services/beta-client.nix`
generates, and connects to `wss://beta.weblounge.org/socket`. Members
are still sent to gamja.

## Bots

A bot is an Ergo account a member owns. Ergo can't tell it from anyone
else's account, so concierge's `bots` table is the record of which
accounts are bots and whose they are. Members make, reset and delete
their own on the members' site's Bots tab, three at a time once they've
confirmed their email; admins use `concierge-admin bot …`, which shares
its code and isn't held to the three. A bot's generated password is
shown once. Deleting a bot erases its Ergo account, disconnecting it.
Its owner may make a bot under its name again at once; anyone else,
member or newcomer, once Ergo's chat history has expired for it, so
nobody passes for it in the scrollback. Concierge is handed that
period, `history.restrictions.expire-time`, from Ergo's settings. Bots
set mode `+B` on themselves.

`templates/bot` is a Go bot to start from (`nix flake init -t
github:luizribeiro/weblounge#bot`); its README covers running it with and
without NixOS. bender, our own bot, doesn't run it: on credenza,
[louro](https://github.com/luizribeiro/louro) holds bender's connection,
in #bots and #ops, and answers nothing. louro may speak in #ops alone,
and only members of the `louro` group may use its socket. Before each
start, `bender-account.service` registers bender as luiz's bot, or puts
back its generated password, so it stays down until luiz is a member.

Bots connect from their owner's vhost, `<bot>!~u@weblounge/bot/<owner>`,
where members show `<nick>!~u@weblounge`; Ergo's API sets it through
`pkgs/ergochat-hs-set-api.patch`. `bot create`, `ensure` and
`reset-password` set it. Bots from before vhosts need `concierge-admin
bot apply-vhosts` once after the deploy that brought them; its help text
marks it as a migration, and running it again changes nothing. Bots stay
out of #lounge, which everyone auto-joins, through its one ban
`+b *!*@weblounge/bot/*`, which luiz sets as the channel's founder.

## The greeter

`concierge`, weblounge's resident bot, is an account of the network's own,
neither a member nor a member's bot: nobody may take the name, and `member
reconcile` expects it. `crates/greeter` connects as it, in bot mode,
from `concierge!~u@weblounge/concierge`, outside the bots' ban, so it
sits in #lounge. Every invite a member made records an arrival when
it's redeemed (root invites, which bootstrap the network, don't), and
every few seconds the greeter WHOISes the newcomers it hasn't seen
connected yet. At first sight it welcomes them in #lounge, DMs their
inviter and says hello to them; it nudges the inviter of one who
hasn't connected two days in. `welcome_rules.rs` holds the timings;
each step is recorded once Ergo echoed it, and the records go two
weeks after joining, along with those of a removed member's invitees.

It parses what the network sends it, so it holds concierge's database
and its account's password alone. `concierge-greeter-account.service`,
which holds Ergo's API token, puts the generated password back before
every start. VM tests turn the greeter off, but for `welcome.py`.

## Uploads

Members paste or attach images in IRC clients, which upload them to the
endpoint Ergo advertises in the `soju.im/FILEHOST` and `draft/FILEHOST`
ISUPPORT tokens, `https://files.weblounge.org/upload`, and post the link
they get back. The limits and the retention live in
`crates/concierge/src/upload_rules.rs`: 10 MB an image, 500 MB a member,
their bots' counted to them, 30 an hour an account, deleted after 30
days.

Two processes share the work, so the one that parses what members
upload holds nothing else:

1. Concierge authenticates the upload with the credentials the client
   logs in to IRC with: gamja's access token as a Bearer, or a nick and
   password as Basic, which Ergo checks behind the sign-in limits. It
   checks the limits, and records each upload in its `uploads` table.
2. `crates/scrubber`, a binary of its own, does everything with the
   bytes. `concierge-scrubber.socket` starts one per connection, with
   no network, a dynamic user, and nothing of the filesystem but the
   store and the uploads directory. Concierge streams the body to it,
   in the frames `crates/filehost` defines; it tells PNG, JPEG, GIF and
   WebP from their bytes, refuses anything else, strips EXIF (but for
   the orientation), XMP, comments and the like without decoding a
   pixel, and stores what's left in `/var/lib/concierge-files` under a
   random 22-character id and the extension of the type it found.

nginx serves the files from `files.weblounge.org`, logging nothing, as
images that nothing may run or index. Members see and delete their
uploads, and their bots', on the Uploads tab. `concierge-expire-uploads`
deletes them after 30 days, hourly, with any file no upload stands for;
`member delete` and `bot delete` delete them with their owner.

## Running a server locally

`just vm start` boots credenza's real configuration in a QEMU VM on your
machine, with ports forwarded from localhost:

| Service | Address                                          |
| ------- | ------------------------------------------------ |
| SSH     | `just vm ssh`                                    |
| HTTP    | http://weblounge.localhost:10080                 |
| HTTPS   | https://weblounge.localhost:10443 (self-signed)  |
| Members | https://members.weblounge.localhost:10443        |
| Chat    | https://irc.weblounge.localhost:10443            |
| IRC     | irc.weblounge.localhost:16697, TLS (self-signed) |

The VM's names live under `weblounge.localhost`, which browsers resolve to
your machine without any `/etc/hosts` edits.

IRC requires SASL, and there's no NickServ registration: accounts come
from Ergo's API on the server's loopback, authenticated with the token
in `/run/weblounge-secrets/ergo-api-token`.

The VM's console is attached to your terminal (Ctrl-A X quits QEMU). From
another terminal:

```sh
just vm push              # deploy your working tree into the running VM, no reboot
just vm ssh               # shell into it
just vm concierge status  # run a concierge admin command in it
just vm reset             # drop its disk so the next start is fresh
```

`just vm push` deploys exactly the way `just prod push` does, so it doubles
as a rehearsal for production deploys.

Logging in requires one of the SSH keys in `modules/admins.nix`.

## Production

```sh
just prod build            # build credenza's system closure
just prod install root@IP  # first install on a fresh server (wipes its disk)
just prod push             # deploy to credenza.weblounge.org
```

`just prod push` refuses to deploy while deSEC's zone differs from the one
the hosts generate (`just dns push` first), and waits until every
nameserver answers the names it orders certificates for.

Every push, to production or the VM, runs the new system on trial. It keeps
it only if a fresh SSH login works and every deploy check passes; otherwise
it switches back to the system that was running. Deploy checks are declared
next to what they check, as `weblounge.deployChecks`, and run on the host or
from your machine. The host keeps a timer of its own, so it also goes back
by itself, ten minutes on, if the push loses touch with it. #ops hears of
each push once it's built, before the copy to the host, and then of its
trial and whether it was kept, rolled back or given up on before switching.

IRC clients connect to `irc.weblounge.org:6697` over TLS.

Concierge's admin commands run on the server, as the service itself:
`concierge-admin status` over SSH, or `just prod concierge status` from your
machine. They print plain lines a script can read:

| Command                                         | What it does                                                     |
| ----------------------------------------------- | ---------------------------------------------------------------- |
| `status`                                        | Configuration, secrets, database and whether Ergo answers        |
| `invite create --root [--admin]`                | Prints a root invite's link; `--admin` makes its redeemer admin  |
| `member reset-password <account>`               | Sets a new generated password, ends sessions, prints it once     |
| `member delete <account>`                       | Locks a member out, removes data, uploads and bots; place stays  |
| `member reconcile`                              | Lists accounts Ergo and concierge disagree on, bots included     |
| `member send-verification <account>`            | Mails a member a new link to confirm their email                 |
| `member send-verification --unverified`         | Mails one to every unconfirmed member never sent one, 60 a run   |
| `bot create <owner> <name> [--note …]`          | Registers a member's bot, prints its generated password once     |
| `bot list [<owner>]`                            | Lists bots, of everyone or one member: name, owner, date, note   |
| `bot reset-password <name>`                     | Sets a bot's new generated password, prints it once              |
| `bot delete <name>`                             | Erases a bot and its uploads, disconnecting it; frees its name   |
| `bot ensure <owner> <name> --password-file <f>` | Creates the bot if missing, else sets its password; `-` is stdin |

After `member delete`, an oper still runs `/NS ERASE <account>`. `bot
delete` erases the bot's account itself, through
`pkgs/ergochat-ns-erase-api.patch`, which disconnects it too.

`member send-verification` is for the members who joined before emails
were confirmed, and goes once they all have. Members have a week from
their first link to confirm; `concierge-remind.timer` runs `concierge
remind` hourly, which mails the one reminder, with a fresh link, from the
fifth day.

## DNS

deSEC hosts the `weblounge.org` zone, and this repository is its source
of truth. `dns/zone.nix` generates it from the hosts: an A record for
each host's own name, and a record for every name its nginx serves, so a
new virtual host gets its DNS without anyone writing a record. Mail for
the domain goes through MXroute, whose MX, SPF, DKIM and DMARC records
the zone also holds.

```sh
just dns preview   # what would change at deSEC
just dns push      # make deSEC match the repository
```

Both run dnscontrol with the deSEC API token from `secrets/dns.yaml`,
which sops decrypts with luiz's GPG key; `sops secrets/dns.yaml` edits
it. `push` also deletes records the repository doesn't have.

## Secrets

Most secrets are random, generated on the machine by
`weblounge-secrets.service`. The ones services only read as they start,
like Ergo's API token, live in RAM, in `/run/weblounge-secrets`, and
every boot makes new ones, so a copy of the disk holds nothing that
works; the rest, like the session key that keeps members signed in,
stay in `/var/lib/weblounge-secrets`. Those that come from outside,
like the SMTP password, live in `secrets/<host>.yaml`, encrypted to
luiz's GPG key and the host's SSH host key; sops-nix decrypts them on
the host. `sops secrets/credenza.yaml` edits them. VMs decrypt stand-ins
in `secrets/vm.yaml` with `secrets/vm-age.key`, committed on purpose.

## Layout

| Path         | What lives there                                                  |
| ------------ | ----------------------------------------------------------------- |
| `hosts/`     | One directory per machine; each becomes a nixosConfiguration      |
| `modules/`   | Configuration shared by every host                                |
| `lib/`       | Host discovery and per-host flake outputs                         |
| `tests/`     | Flake checks of the Nix code; `tests/e2e/` holds the VM tests     |
| `services/`  | Opt-in configuration, one file per service a host can run         |
| `pkgs/`      | Our own packages, exposed to hosts as `pkgs.weblounge.*`          |
| `crates/`    | Rust code: concierge, its mailer, outbox and scrubber, ergo-api   |
| `web/`       | Sources of everything the browser loads                           |
| `templates/` | `nix flake init` templates; `bot` is the template bots start from |
| `dns/`       | The DNS zone, generated from the hosts, and its dnscontrol config |
| `just/`      | `just vm …`, `just prod …`, `just dns …`, `just web …` and more   |
| `scripts/`   | Helpers the recipes call                                          |

Servers are named after furniture: `credenza` is the first.
