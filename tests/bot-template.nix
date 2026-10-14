# The bot template's own VM test, against an Ergo configured like ours.
{ pkgs, weblounge, ... }:

import ../templates/bot/test {
  pkgs = pkgs.extend weblounge.overlay;
  ergo = weblounge.testErgo;
}
