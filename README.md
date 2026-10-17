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
at compile time against the metadata in `crates/concierge/.sqlx`. After
changing a query or adding a migration to `crates/concierge/migrations`,
run `just concierge prepare` and commit what it writes.

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

## Bots

A bot is an Ergo account a member owns. Ergo can't tell it from anyone
else's account, so concierge's `bots` table is the record of which
accounts are bots and whose they are; admins create them with
`concierge-admin bot create`. Bots set mode `+B` on themselves.

`templates/bot` is a Go bot to start from (`nix flake init -t
github:luizribeiro/weblounge#bot`); its README covers running it with and
without NixOS. bender, our own bot, is that template deployed as is on
credenza: it joins #bots and answers `!ping`. Before each start,
`bender-account.service` registers it as luiz's bot, or puts back its
generated password, so it stays down until luiz is a member.

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
in `/var/lib/weblounge-secrets/ergo-api-token`.

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
by itself, ten minutes on, if the push loses touch with it.

IRC clients connect to `irc.weblounge.org:6697` over TLS.

Concierge's admin commands run on the server, as the service itself:
`concierge-admin status` over SSH, or `just prod concierge status` from your
machine. They print plain lines a script can read:

| Command                                         | What it does                                                     |
| ----------------------------------------------- | ---------------------------------------------------------------- |
| `status`                                        | Configuration, secrets, database and whether Ergo answers        |
| `invite create --root [--admin]`                | Prints a root invite's link; `--admin` makes its redeemer admin  |
| `member reset-password <account>`               | Sets a new generated password, ends sessions, prints it once     |
| `member delete <account>`                       | Locks a member out, removes data and bots; their place stays     |
| `member reconcile`                              | Lists accounts Ergo and concierge disagree on, bots included     |
| `bot create <owner> <name> [--note …]`          | Registers a member's bot, prints its generated password once     |
| `bot list [<owner>]`                            | Lists bots, of everyone or one member: name, owner, date, note   |
| `bot reset-password <name>`                     | Sets a bot's new generated password, prints it once              |
| `bot delete <name>`                             | Locks a bot out and marks it deleted, keeping its name taken     |
| `bot ensure <owner> <name> --password-file <f>` | Creates the bot if missing, else sets its password; `-` is stdin |

Ergo's API can't delete accounts, so after `member delete` or `bot delete`
an oper still runs `/NS ERASE <account>`.

## DNS

deSEC hosts the `weblounge.org` zone, and this repository is its source
of truth. `dns/zone.nix` generates it from the hosts: an A record for
each host's own name, and a record for every name its nginx serves, so a
new virtual host gets its DNS without anyone writing a record. The
domain sends and receives no mail, and the zone says so.

```sh
just dns preview   # what would change at deSEC
just dns push      # make deSEC match the repository
```

Both run dnscontrol with the deSEC API token from `secrets/dns.yaml`,
which sops decrypts with luiz's GPG key; `sops secrets/dns.yaml` edits
it. `push` also deletes records the repository doesn't have.

## Layout

| Path         | What lives there                                                  |
| ------------ | ----------------------------------------------------------------- |
| `hosts/`     | One directory per machine; each becomes a nixosConfiguration      |
| `modules/`   | Configuration shared by every host                                |
| `lib/`       | Host discovery and per-host flake outputs                         |
| `tests/`     | Flake checks of the Nix code; `tests/e2e/` holds the VM tests     |
| `services/`  | Opt-in configuration, one file per service a host can run         |
| `pkgs/`      | Our own packages, exposed to hosts as `pkgs.weblounge.*`          |
| `crates/`    | Rust code: concierge, and ergo-api, its client for Ergo's API     |
| `web/`       | Sources of everything the browser loads                           |
| `templates/` | `nix flake init` templates; `bot` is the template bots start from |
| `dns/`       | The DNS zone, generated from the hosts, and its dnscontrol config |
| `just/`      | `just vm …`, `just prod …`, `just dns …`, `just web …` and more   |
| `scripts/`   | Helpers the recipes call                                          |

Servers are named after furniture: `credenza` is the first.
