# weblounge

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
stylesheet, `web/styles/members.css`, builds on the landing page's tokens,
and esbuild gives every asset a hashed name that concierge looks up in the
build's manifest.

```sh
just concierge dev   # run it on http://localhost:8080
```

bacon restarts concierge whenever its code, a template or a stylesheet
changes, and the open page reloads once it's back. `cargo test` runs its
tests; `just check` adds clippy and rustfmt.

## Running a server locally

`just vm start` boots credenza's real configuration in a QEMU VM on your
machine, with ports forwarded from localhost:

| Service | Address                                          |
| ------- | ------------------------------------------------ |
| SSH     | `just vm ssh`                                    |
| HTTP    | http://weblounge.localhost:10080                 |
| HTTPS   | https://weblounge.localhost:10443 (self-signed)  |
| Members | https://members.weblounge.localhost:10443        |
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

IRC clients connect to `irc.weblounge.org:6697` over TLS.

Concierge's admin commands run on the server, as the service itself:
`concierge-admin status` over SSH, or `just prod concierge status` from your
machine.

## Layout

| Path        | What lives there                                                |
| ----------- | --------------------------------------------------------------- |
| `hosts/`    | One directory per machine; each becomes a nixosConfiguration    |
| `modules/`  | Configuration shared by every host                              |
| `lib/`      | Host discovery and per-host flake outputs                       |
| `tests/`    | NixOS VM tests, run by `just check`                             |
| `services/` | Opt-in configuration, one file per service a host can run       |
| `pkgs/`     | Our own packages, exposed to hosts as `pkgs.weblounge.*`        |
| `crates/`   | Rust code: concierge                                            |
| `web/`      | Sources of everything the browser loads                         |
| `just/`     | `just vm …`, `just prod …`, `just web …` and `just concierge …` |
| `scripts/`  | Helpers the recipes call                                        |

Servers are named after furniture: `credenza` is the first.
