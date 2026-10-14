{ lib, ... }:

let
  admins = {
    luiz = {
      keys = [
        "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAACAQDOhhBeexzQlvA05Vt0BTx3AHYEwgjbH3RcaQKQD1StD2bv7Xh4dr6Xu00trLBCLCdYSuJUlF00Oj55b3VNPaXUIAjy/I/aXbb0iINx/C2ieICaQi1efQh0M6Dbc8rFwnJJb11Ou/j/3BEBz8DpWOQnv9o5Q01z5PpWQPkOnVzDPbn54sB8HhEWGRW1dgqZvwWh0JCU2xhEset/FYna01kCYIZ+TIpEGWw97XzQCUirsyKtokXT45KOl+/M84ZmsAM7eShKkY3ypzGhBUiOTfJ8HgBcYiuFSeYPMhou2S0Octi5D5V+12nM/FAGqzesbdSQMxoAgPZt69XJVaWcDtEGE3cGT5h7PBhkEpix0957rcMrSR0mv+q1kOvwqT3zWP1fFcmqMVHxrzOtCpu40zXyDMIXZu71jxJhhtKzssOtfSmU6GVO0ldTGsNuc2rIXE+ekflrDtq3zphWhV/T/irRdiJxDaaFov60SdmlqFC+gwBRktavM2nCv3yAPqydlxUWCSeeB+elaXBhezOveJUO/Y3rZR2f4CaHYOaeAukNRUjsfKH+kTEbOcyFNM3YVZoJcsrJyuw76sR32YYYMXJdaT09N+v+HUpi95Q91PUeV2X0t1IjyIRJxxvlCiq6YmrKjiWTdTwm076zW+6i5L91dLZW6vCR1fA8zLVoFie6DQ== cardno:27_054_871"
      ];
      # For the hosting provider's VNC console only; sshd refuses passwords.
      hashedPassword = "$y$j9T$h1YZ6f5ewtq/dArJ3AjVc/$ewaUrUWCkephQILIBKqElNkc3mwJ8swxZ4IEMcp6GZ/";
    };
  };
in
{
  users.mutableUsers = false;

  services.openssh.settings.AllowUsers = lib.attrNames admins;

  users.users = lib.mapAttrs
    (_: admin: {
      isNormalUser = true;
      extraGroups = [ "wheel" ];
      openssh.authorizedKeys.keys = admin.keys;
      hashedPassword = admin.hashedPassword or null;
    })
    admins;
}
