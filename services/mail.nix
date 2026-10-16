{
  imports = [ ./secrets.nix ];

  # The password of the noreply@ mailbox, which mail is sent from.
  weblounge.secrets.smtp-password.fromSops = true;
}
