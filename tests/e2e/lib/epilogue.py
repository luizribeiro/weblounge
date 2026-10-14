from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import subtest
    from helpers import assert_client_address_absent, client_addresses, files_containing

# sshd's log is the admins' record of who got into the server; it names
# admins and scanners, never members.
with subtest("after everything above, only sshd's log names a client address"):
    assert_client_address_absent("journalctl | grep -vE ' sshd(-session|-auth)?\\[[0-9]+\\]: '")

with subtest("after everything above, no file on credenza names a client address"):
    found = files_containing(*client_addresses())
    assert found == [], found
