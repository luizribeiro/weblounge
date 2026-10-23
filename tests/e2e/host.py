import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, mail, subtest, tools
    from helpers import add_member, ergo_api_token

# As in host.nix, whose stand-in certificate order fails its first attempt.
cert = "files.weblounge.org"
attempted = f"/var/lib/acme/.attempted-{cert}"

with subtest("sshd only accepts keys"):
    credenza.wait_for_unit("sshd.service")
    config = credenza.succeed("sshd -T").lower()
    assert "passwordauthentication no" in config, config
    assert "permitrootlogin no" in config, config

with subtest("sshd lets admins alone in, and forwards nothing for them"):
    config = credenza.succeed("sshd -T").lower().splitlines()
    for setting in [
        "allowusers luiz",
        "allowtcpforwarding no",
        "allowagentforwarding no",
        "allowstreamlocalforwarding no",
        "maxauthtries 3",
        "logingracetime 30",
    ]:
        assert setting in config, (setting, config)

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


def unit_file_problems(*paths: str) -> list[str]:
    """What systemd-analyze objects to in these unit files, beyond upstream units that are masked or name
    programs left out of this system."""
    _, output = credenza.execute(f"systemd-analyze verify --man=no --recursive-errors=no {' '.join(paths)} 2>&1")
    return [line for line in output.splitlines() if not re.search(r"is masked|is not executable", line)]


with subtest("systemd knows every setting in credenza's units, so no sandbox option is silently dropped"):
    credenza.succeed(
        "printf '[Service]\\nExecStart=/run/current-system/sw/bin/true\\nProtecSystem=strict\\n' > /tmp/typo.service"
    )
    assert any("Unknown key 'ProtecSystem'" in line for line in unit_file_problems("/tmp/typo.service"))
    units = credenza.succeed("ls /etc/systemd/system/*.service /etc/systemd/system/*.timer").split()
    assert unit_file_problems(*units) == []


with subtest("the deploy checks pass on a healthy credenza, and name a unit that failed"):
    mail.wait_for_open_port(465)
    add_member("luiz", "plain-old-vanilla")
    credenza.succeed("systemctl restart bender-account.service louro.service")
    credenza.succeed("/run/current-system/deploy-checks-host")
    credenza.succeed("systemd-run --unit=doomed false")
    status, report = credenza.execute("DEPLOY_CHECKS_TIMEOUT=3 /run/current-system/deploy-checks-host 2>&1")
    assert status == 1 and "✗ no unit failed or is waiting to retry" in report and "doomed.service" in report, report
    credenza.succeed("systemctl reset-failed doomed.service")

with subtest("the deploy checks hand Ergo's API token to curl off its command line, which anyone may read"):
    credenza.succeed(
        f"{tools['strace']} -f -qq -e trace=execve -s 65536 -o /tmp/execs /run/current-system/deploy-checks-host"
    )
    execs = credenza.succeed("cat /tmp/execs")
    assert "/bin/curl" in execs and ergo_api_token() not in execs, execs

with subtest("the mailer's deploy check fails while the SMTP password is wrong, and says why it may be"):
    password = "/run/secrets/smtp-password"
    credenza.succeed(f"cp {password} /tmp/smtp-password && echo not-the-password > {password}")
    status, report = credenza.execute("/run/current-system/deploy-checks-host 2>&1")
    credenza.succeed(f"cp /tmp/smtp-password {password}")
    assert status == 1 and "✗ the mailer runs and logs in to its SMTP server" in report, report
    assert "Can't log in to mail:465 as noreply@weblounge.org: SMTP 535" in report, report
    assert "the real password replaces the placeholder" in report, report
    credenza.succeed("/run/current-system/deploy-checks-host")


with subtest("secrets from sops are decrypted for root alone"):
    assert credenza.succeed("cat /run/secrets/smtp-password") == "vm-smtp-password"
    assert credenza.succeed("stat -L -c '%U %a' /run/secrets/smtp-password").strip() == "root 400"
