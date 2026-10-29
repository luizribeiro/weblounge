"""Connect to an IRC server over TLS, or over a WebSocket the way a web
client does, optionally logging in with SASL PLAIN, EXTERNAL or
OAUTHBEARER, send COMMANDS once welcomed, and print every line the server
sends until it goes quiet."""

import argparse
import base64
import contextlib
import os
import socket
import ssl

QUIET_SECONDS = 2


def connect(host, port, name=None, cert=None):
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    if cert:
        context.load_cert_chain(cert)
    sock = socket.create_connection((host, port), timeout=10)
    return context.wrap_socket(sock, server_hostname=name or host)


class WebSocket:
    """Just enough of RFC 6455 to carry IRC lines, one per text frame, as
    the text.ircv3.net subprotocol has it. Stands in for a socket."""

    def __init__(self, sock, name, path="/socket"):
        self.sock = sock
        key = base64.b64encode(os.urandom(16)).decode()
        request = [
            f"GET {path} HTTP/1.1",
            f"Host: {name}",
            f"Origin: https://{name}",
            "Upgrade: websocket",
            "Connection: Upgrade",
            f"Sec-WebSocket-Key: {key}",
            "Sec-WebSocket-Version: 13",
            "Sec-WebSocket-Protocol: text.ircv3.net",
            "",
            "",
        ]
        sock.sendall("\r\n".join(request).encode())
        self.pending = b""
        while b"\r\n\r\n" not in self.pending:
            self.pending += sock.recv(4096) or b"\r\n\r\n"
        head, self.pending = self.pending.split(b"\r\n\r\n", 1)
        status = head.split(b"\r\n")[0].decode()
        if " 101 " not in status:
            raise SystemExit(f"WebSocket upgrade refused: {status}")

    def settimeout(self, seconds):
        self.sock.settimeout(seconds)

    def sendall(self, line):
        payload = line.removesuffix(b"\r\n")
        assert len(payload) < 126
        mask = os.urandom(4)
        masked = bytes(c ^ mask[i % 4] for i, c in enumerate(payload))
        self.sock.sendall(bytes([0x81, 0x80 | len(payload)]) + mask + masked)

    def recv(self, _size):
        """The next text frame as a line; b"" once the server closes."""
        try:
            return self.text_frame()
        except EOFError:
            return b""

    def text_frame(self):
        while True:
            first, second = self.read(2)
            length = second & 0x7F
            if length >= 126:
                width = 2 if length == 126 else 8
                length = int.from_bytes(self.read(width), "big")
            payload = self.read(length)
            opcode = first & 0x0F
            if opcode == 0x8:
                return b""
            if opcode == 0x1:
                return payload + b"\r\n"

    def read(self, size):
        while len(self.pending) < size:
            data = self.sock.recv(4096)
            if not data:
                raise EOFError
            self.pending += data
        data, self.pending = self.pending[:size], self.pending[size:]
        return data


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
    encoded = base64.b64encode(payload.encode()).decode()
    conn.send("AUTHENTICATE " + (encoded or "+"))
    if conn.read_until("903", "904") != "903":
        raise SystemExit(f"SASL {mechanism} login failed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("host")
    parser.add_argument("--account")
    parser.add_argument("--password")
    parser.add_argument("--token", help="an OAuth 2.0 bearer token")
    parser.add_argument("--nick", help="the nick to ask for; the account's, or visitor, by default")
    parser.add_argument("--cert", help="a PEM file with the key and certificate to present")
    parser.add_argument("--websocket", metavar="NAME", help="connect to wss://NAME/socket at the host, as a web client")
    parser.add_argument(
        "--linger",
        type=int,
        default=QUIET_SECONDS,
        metavar="SECONDS",
        help="stay connected after the commands until the server is quiet this long or closes the connection",
    )
    parser.add_argument("commands", nargs="*")
    args = parser.parse_args()
    if args.account and not (args.password or args.cert):
        parser.error("--account needs --password or --cert")

    if args.websocket:
        sock = connect(args.host, 443, args.websocket)
        conn = Connection(WebSocket(sock, args.websocket))
    else:
        conn = Connection(connect(args.host, 6697, cert=args.cert))
    conn.send("CAP LS 302")
    if args.cert:
        authenticate(conn, "EXTERNAL", "")
    elif args.account:
        plain = f"{args.account}\0{args.account}\0{args.password}"
        authenticate(conn, "PLAIN", plain)
    elif args.token:
        bearer = f"n,,\x01auth=Bearer {args.token}\x01\x01"
        authenticate(conn, "OAUTHBEARER", bearer)
    conn.send("CAP END")
    conn.send(f"NICK {args.nick or args.account or 'visitor'}")
    conn.send("USER guest 0 * :weblounge test client")
    if conn.read_until("001") is None:
        return

    for command in args.commands:
        conn.send(command)
    conn.drain(args.linger)
    with contextlib.suppress(OSError):
        conn.send("QUIT")


if __name__ == "__main__":
    main()
