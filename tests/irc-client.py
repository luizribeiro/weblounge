"""Connect to an IRC server over TLS, optionally logging in with SASL PLAIN
or OAUTHBEARER, send COMMANDS once welcomed, and print every line the
server sends."""

import argparse
import base64
import socket
import ssl

QUIET_SECONDS = 2


def connect(host, port):
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    sock = socket.create_connection((host, port), timeout=10)
    return context.wrap_socket(sock, server_hostname=host)


class Connection:
    def __init__(self, sock):
        self.sock = sock
        self.buffer = b""

    def send(self, line):
        self.sock.sendall(line.encode() + b"\r\n")

    def lines(self):
        while True:
            while b"\r\n" in self.buffer:
                line, self.buffer = self.buffer.split(b"\r\n", 1)
                line = line.decode(errors="replace")
                print(line, flush=True)
                if line.startswith("PING "):
                    self.send("PONG " + line[5:])
                yield line
            try:
                data = self.sock.recv(4096)
            except TimeoutError:
                return
            if not data:
                return
            self.buffer += data

    def drain(self, seconds):
        self.sock.settimeout(seconds)
        for _ in self.lines():
            pass

    def read_until(self, *numerics):
        for line in self.lines():
            words = line.split()
            if len(words) > 1 and words[1] in numerics:
                return words[1]
        return None


def authenticate(conn, mechanism, payload):
    conn.send("CAP REQ :sasl")
    conn.send(f"AUTHENTICATE {mechanism}")
    conn.send("AUTHENTICATE " + base64.b64encode(payload.encode()).decode())
    if conn.read_until("903", "904") != "903":
        raise SystemExit(f"SASL {mechanism} login failed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("host")
    parser.add_argument("--account")
    parser.add_argument("--password")
    parser.add_argument("--token", help="an OAuth 2.0 bearer token")
    parser.add_argument("commands", nargs="*")
    args = parser.parse_args()
    if args.account and not args.password:
        parser.error("--account needs --password")

    conn = Connection(connect(args.host, 6697))
    conn.send("CAP LS 302")
    if args.account:
        plain = f"{args.account}\0{args.account}\0{args.password}"
        authenticate(conn, "PLAIN", plain)
    elif args.token:
        bearer = f"n,,\x01auth=Bearer {args.token}\x01\x01"
        authenticate(conn, "OAUTHBEARER", bearer)
    conn.send("CAP END")
    conn.send(f"NICK {args.account or 'visitor'}")
    conn.send("USER guest 0 * :weblounge test client")
    if conn.read_until("001") is None:
        return

    for command in args.commands:
        conn.send(command)
    conn.drain(QUIET_SECONDS)
    conn.send("QUIT")


if __name__ == "__main__":
    main()
