# What the VM tests' Python imports, for ty to check it against: the test
# driver that runs the test scripts, and Playwright for the browser scripts.
{ pkgs }:

pkgs.python3.withPackages (ps: [ (ps.toPythonModule pkgs.nixos-test-driver) ps.playwright ])
