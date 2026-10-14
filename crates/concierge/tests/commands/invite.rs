use concierge::invite_rules::LIFETIME;

use crate::Fixture;

#[tokio::test]
async fn create_root_prints_a_link_to_a_new_invite_nobody_vouches_for() {
    let fixture = Fixture::start().await;

    let output = fixture.run(&["invite", "create", "--root"]).await.unwrap();

    let code = output
        .strip_prefix(&fixture.config.invite_url(""))
        .and_then(|rest| rest.strip_suffix('\n'))
        .unwrap_or_else(|| panic!("{output:?}"));
    assert_eq!(code.len(), 22, "{output}");
    let invite = fixture.db().await.invite(code).await.unwrap().unwrap();
    assert_eq!(invite.created_by, None);
    assert_eq!(invite.expires_at, invite.created_at + LIFETIME);
    assert_eq!(
        (invite.claimed_at, invite.redeemed_by, invite.revoked_at),
        (None, None, None)
    );
}

#[tokio::test]
async fn every_invite_gets_its_own_code() {
    let fixture = Fixture::start().await;
    let first = fixture.run(&["invite", "create", "--root"]).await.unwrap();
    let second = fixture.run(&["invite", "create", "--root"]).await.unwrap();
    assert_ne!(first, second);
}

#[tokio::test]
async fn only_root_invites_are_made_on_the_command_line() {
    let fixture = Fixture::start().await;
    let error = fixture.run(&["invite", "create"]).await.unwrap_err();
    assert!(error.to_string().contains("--root"), "{error}");
}
