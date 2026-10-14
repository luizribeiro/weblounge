from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, subtest

# As in host.nix, whose stand-in certificate order fails its first attempt.
cert = "irc.weblounge.org"
attempted = f"/var/lib/acme/.attempted-{cert}"

with subtest("sshd only accepts keys"):
    credenza.wait_for_unit("sshd.service")
    config = credenza.succeed("sshd -T").lower()
    assert "passwordauthentication no" in config, config
    assert "permitrootlogin no" in config, config

with subtest("admins log in with their key and get passwordless sudo"):
    credenza.succeed("id -nG luiz | grep -qw wheel")
    credenza.succeed("grep -q cardno: /etc/ssh/authorized_keys.d/luiz")
    credenza.succeed("su - luiz -c 'sudo -n true'")
    credenza.succeed("getent shadow luiz | cut -d: -f2 | grep -q '^\\$y\\$'")

with subtest("the VPS's NIC comes up with its static address and gateway"):
    credenza.wait_until_succeeds("ip -4 addr show | grep -q 'inet 107.173.231.206/25'", timeout=60)
    credenza.succeed("ip route show default | grep -q 'via 107.173.231.129'")

with subtest("a certificate order the CA refuses is tried again, not left for the daily renewal"):
    unit = f"acme-order-renew-{cert}.service"
    credenza.wait_until_succeeds(f"test -e {attempted}", timeout=60)
    credenza.wait_until_succeeds(f"systemctl show -P Result {unit} | grep -qx success", timeout=60)
    assert credenza.succeed(f"systemctl show -P NRestarts {unit}").strip() == "1"

with subtest("the kernel keeps io_uring, its addresses, ptrace and kexec from services"):
    for setting, value in {
        "kernel.io_uring_disabled": "2",
        "kernel.kptr_restrict": "2",
        "dev.tty.ldisc_autoload": "0",
        "kernel.yama.ptrace_scope": "2",
        "kernel.kexec_load_disabled": "1",
    }.items():
        assert credenza.succeed(f"sysctl -n {setting}").strip() == value, setting

with subtest("credenza neither follows nor sends ICMP redirects, on any interface"):
    settings = credenza.succeed("sysctl -a -r '\\.(accept|send)_redirects$'").strip().splitlines()
    assert len(settings) > 6 and all(line.endswith(" = 0") for line in settings), settings
