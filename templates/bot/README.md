# A weblounge.org bot

A small IRC bot in Go, on [irc-go](https://github.com/ergochat/irc-go). It
logs in to `irc.weblounge.org` with its own account, marks itself as a bot
(`+B`), joins its channels and answers `!ping` with `pong`: `nick: pong` in
a channel, `pong` in a private message, threaded under the ping.

Add commands in `onMessage` in `main.go`; `reply()` sends an answer back
where the question was asked.

## An account

Every bot needs an account of its own. Make one on the Bots tab at
[members.weblounge.org/bots](https://members.weblounge.org/bots); its
password is shown once. The bot's nick is its account name.

Bots can't join #lounge. Use #bots, or a channel of your own.

## Configuration

The bot reads its environment:

| Variable            | What it is                                                    |
| ------------------- | ------------------------------------------------------------- |
| `IRC_NICK`          | The bot's account name and nick (required)                    |
| `IRC_CHANNELS`      | Channels to join, separated by commas, e.g. `#bots,#robots`   |
| `IRC_SERVER`        | `host:port`, over TLS; defaults to `irc.weblounge.org:6697`   |
| `IRC_PASSWORD_FILE` | A file holding the password                                   |
| `IRC_CA_FILE`       | A CA to trust besides the system's, for a test server         |

The password is never read from the environment itself. Under systemd's
`LoadCredential=password:…` the bot reads `$CREDENTIALS_DIRECTORY/password`
and ignores `IRC_PASSWORD_FILE`.

## With Nix

```sh
mkdir my-bot && cd my-bot
nix flake init -t github:luizribeiro/weblounge#bot
nix develop        # go and gopls
go test ./...
nix build          # ./result/bin/bot, statically linked
nix flake check    # go test, plus a VM test against a local Ergo
```

After changing `go.mod`, set `vendorHash` in `package.nix` to
`lib.fakeHash`, run `nix build`, and copy the hash it reports.

On NixOS, import the flake's `nixosModules.default` (or `module.nix`) and
declare the bot:

```nix
services.weblounge-bot.bender = {
  enable = true;
  channels = [ "#bots" ];
  passwordFile = "/run/secrets/bender-password";
};
```

`passwordFile` is a path string, so the password stays out of the Nix
store; put it there with agenix, sops-nix or by hand, readable by root
only. The bot runs as a sandboxed `DynamicUser` service named
`weblounge-bot-bender`.

## Without Nix

With Go 1.24 or later:

```sh
go test ./...
CGO_ENABLED=0 go build -o bot .
IRC_NICK=bender IRC_CHANNELS='#bots' IRC_PASSWORD_FILE=~/.config/bender/password ./bot
```

To build a static binary for another machine, say a 64-bit ARM server:

```sh
CGO_ENABLED=0 GOOS=linux GOARCH=arm64 go build -trimpath -ldflags='-s -w' -o bot-linux-arm64 .
```

### A systemd unit

Copy the binary to `/usr/local/bin/bot`, the password to
`/etc/bender/password` (owned by root, mode 600), and write
`/etc/systemd/system/bender.service`:

```ini
[Unit]
Description=IRC bot bender
Wants=network-online.target
After=network-online.target

[Service]
ExecStart=/usr/local/bin/bot
Environment=IRC_NICK=bender
Environment=IRC_CHANNELS=#bots
LoadCredential=password:/etc/bender/password
DynamicUser=yes
Restart=on-failure
RestartSec=10
UMask=0077
CapabilityBoundingSet=
LockPersonality=yes
MemoryDenyWriteExecute=yes
NoNewPrivileges=yes
PrivateDevices=yes
PrivateTmp=yes
PrivateUsers=yes
ProcSubset=pid
ProtectClock=yes
ProtectControlGroups=yes
ProtectHome=yes
ProtectHostname=yes
ProtectKernelLogs=yes
ProtectKernelModules=yes
ProtectKernelTunables=yes
ProtectProc=invisible
ProtectSystem=strict
RestrictAddressFamilies=AF_INET AF_INET6
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
SystemCallArchitectures=native
SystemCallFilter=@system-service ~@privileged

[Install]
WantedBy=multi-user.target
```

Then `systemctl enable --now bender`. systemd reads the password as root
and hands it to the bot, which runs as a throwaway user that can't read
`/etc/bender`. The sandbox is the NixOS module's. It leaves out `AF_UNIX`,
which is fine for a binary built with `CGO_ENABLED=0`, since Go then looks
up names over DNS itself.

## In a container

The image holds only the binary and CA certificates, and runs as
`nobody` (uid 65534), so give it a copy of the password it can read:

```sh
install -o 65534 -m 400 /etc/bender/password /etc/bender/container-password
nix build .#image
docker load < result
docker run -d --name bender \
  -e IRC_NICK=bender -e IRC_CHANNELS='#bots' \
  -e IRC_PASSWORD_FILE=/run/secrets/password \
  -v /etc/bender/container-password:/run/secrets/password:ro \
  bot:latest
```
