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

## Running a server locally

`just vm start` boots credenza's real configuration in a QEMU VM on your
machine, with ports forwarded from localhost:

| Service | Address              |
| ------- | -------------------- |
| SSH     | `just vm ssh`        |
| HTTP    | http://localhost:10080 |

The VM's console is attached to your terminal (Ctrl-A X quits QEMU). From
another terminal:

```sh
just vm push       # deploy your working tree into the running VM, no reboot
just vm ssh        # shell into it
just vm reset      # drop its disk so the next start is fresh
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

## Layout

| Path        | What lives there                                            |
| ----------- | ----------------------------------------------------------- |
| `hosts/`    | One directory per machine; each becomes a nixosConfiguration |
| `modules/`  | Configuration shared by every host                          |
| `lib/`      | Host discovery and per-host flake outputs                   |
| `tests/`    | NixOS VM tests, run by `just check`                         |
| `site/`     | The static weblounge.org site                               |
| `just/`     | `just vm …` and `just prod …` subcommands                   |
| `scripts/`  | Helpers the recipes call                                    |

Servers are named after furniture: `credenza` is the first.
