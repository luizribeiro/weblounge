{ config, lib, pkgs, ... }:

let
  inherit (lib) mkOption types;

  checkOptions = {
    options = {
      runs = mkOption {
        type = types.enum [ "host" "outside" ];
        default = "host";
        description = "Where the check runs: on the host, or on the machine pushing to it, which sees what visitors see.";
      };
      script = mkOption {
        type = types.lines;
        description = "Shell that exits 0 once the check passes; what it prints explains a failure.";
      };
      timeout = mkOption {
        type = types.ints.positive;
        default = 60;
        description = "Seconds to keep retrying, for services still settling after a switch.";
      };
    };
  };

  # DEPLOY_CHECKS_TIMEOUT replaces every check's timeout, for tests that
  # expect failures.
  retry = pkgs.writeShellScript "deploy-check" ''
    name=$1 script=$2 deadline=$((SECONDS + ''${DEPLOY_CHECKS_TIMEOUT:-$3}))
    until output=$("$script" 2>&1); do
      if ((SECONDS >= deadline)); then
        printf '✗ %s\n%s\n' "$name" "$output" >&2
        exit 1
      fi
      sleep 2
    done
    printf '✓ %s\n' "$name" >&2
  '';

  # Checks run side by side, so a push waits for the slowest one rather than
  # all of them in turn, and every failure is in the one report.
  runner = side: pkgs.writeShellApplication {
    name = "deploy-checks-${side}";
    text = lib.concatStrings
      (lib.mapAttrsToList
        (name: check: ''
          ${retry} ${lib.escapeShellArg name} ${pkgs.writeShellScript "deploy-check" check.script} ${toString check.timeout} &
        '')
        (lib.filterAttrs (_: check: check.runs == side) config.weblounge.deployChecks)) + ''
      failed=0
      for check in $(jobs -p); do
        wait "$check" || failed=1
      done
      exit "$failed"
    '';
  };
in
{
  options.weblounge.outsideCurl = mkOption {
    type = types.str;
    readOnly = true;
    # Machines with stand-in certificates keep their CA to themselves.
    default = "${pkgs.curl}/bin/curl --silent --show-error --max-time 10"
      + lib.optionalString (config.weblounge.selfSignedCa != null) " --insecure";
    description = "curl as an outside deploy check runs it, verifying certificates wherever a public CA issued them.";
  };

  options.weblounge.deployChecks = mkOption {
    type = types.attrsOf (types.submodule checkOptions);
    default = { };
    description = "Checks a push runs against the system it just activated, going back to the old one if any fails. Each is named for what it expects.";
  };

  config = {
    # In the system itself, so a push runs the checks of the system it pushed.
    system.systemBuilderCommands = ''
      ln -s ${lib.getExe (runner "host")} $out/deploy-checks-host
      ln -s ${lib.getExe (runner "outside")} $out/deploy-checks-outside
    '';

    weblounge.deployChecks."no unit failed or is waiting to retry" = {
      # Certificate orders can run for minutes after a switch.
      timeout = 300;
      script = ''
        pending=$(systemctl list-jobs --no-legend --plain)
        broken=$(systemctl list-units --no-legend --plain --state=failed,auto-restart)
        [ -z "$pending$broken" ] && exit 0
        [ -n "$pending" ] && printf 'still running:\n%s\n' "$pending"
        [ -n "$broken" ] && printf 'failed, or waiting to retry:\n%s\n' "$broken"
        exit 1
      '';
    };
  };
}
