"""The names nixos-test-driver puts in scope of every test script."""

from contextlib import AbstractContextManager

from test_driver.machine import QemuMachine

credenza: QemuMachine
client: QemuMachine

def start_all() -> None: ...
def subtest(name: str) -> AbstractContextManager[None]: ...
