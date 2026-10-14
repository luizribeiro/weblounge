"""Log in to IRC as ACCOUNT, join TARGET if it is a channel, send !ping
to TARGET, and print every line the server sends, tags and all, until BOT
says something after the ping. Fails if BOT stays silent. The last two
PRIVMSGs printed are the ping's echo and BOT's answer."""

import argparse
import base64
import socket
import ssl
import sys

TIMEOUT_SECONDS = 5


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("server", help="host:port, spoken to over TLS")
    parser.add_argument("account")
    parser.add_argument("password")
    parser.add_argument("bot")
    parser.add_argument("target")
    parser.add_argument("--ca", help="a CA to trust instead of the system's")
    args = parser.parse_args()

    host, port = args.server.rsplit(":", 1)
    context = ssl.create_default_context(cafile=args.ca)
    raw = socket.create_connection((host, int(port)), timeout=TIMEOUT_SECONDS)
    sock = context.wrap_socket(raw, server_hostname=host)

    def send(line):
        sock.sendall(line.encode() + b"\r\n")

    plain = f"{args.account}\0{args.account}\0{args.password}"
    send("CAP REQ :sasl message-tags echo-message")
    send("AUTHENTICATE PLAIN")
    send("AUTHENTICATE " + base64.b64encode(plain.encode()).decode())
    send("CAP END")
    send(f"NICK {args.account}")
    send(f"USER {args.account} 0 * :{args.account}")

    pinged = False
    try:
        for line in sock.makefile(encoding="utf-8", newline="\r\n"):
            line = line.rstrip("\r\n")
            print(line, flush=True)
            words = line.split(" ")
            if words[0].startswith("@"):
                words = words[1:]
            source, command = words[0], words[1]
            if source == "PING":
                send("PONG " + command)
            elif command == "001":
                if args.target.startswith("#"):
                    send(f"JOIN {args.target}")
                send(f"PRIVMSG {args.target} :!ping")
            elif line.endswith(f" PRIVMSG {args.target} :!ping"):
                pinged = pinged or source.startswith(f":{args.account}!")
            elif command == "PRIVMSG" and source.startswith(f":{args.bot}!"):
                if pinged:
                    return
    except TimeoutError:
        pass
    sys.exit(f"{args.bot} did not answer")


if __name__ == "__main__":
    main()
