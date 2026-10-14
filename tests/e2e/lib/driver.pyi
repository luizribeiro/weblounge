"""The names nixos-test-driver, and lib.nix, put in scope of every test script."""

from contextlib import AbstractContextManager

from test_driver.machine import QemuMachine

credenza: QemuMachine
client: QemuMachine
tools: dict[str, str]

def start_all() -> None: ...
def subtest(name: str) -> AbstractContextManager[None]: ...
