use concierge::db;

use crate::Fixture;

mod test {
    use super::*;

    #[tokio::test]
    async fn queues_a_message_to_the_address_and_prints_its_id() {
        let fixture = Fixture::start().await;

        let output = fixture
            .run(&["mail", "test", " luiz@example.org "])
            .await
            .unwrap();

        let due = fixture.outbox().await.due(db::now()).await.unwrap();
        assert_eq!(output, format!("queued message {}\n", due[0].id));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].recipient, "luiz@example.org");
        assert_eq!(due[0].subject, "A test message from weblounge.org");
        assert!(
            due[0]
                .body
                .contains("check that the network's mail goes out")
        );
        let output = fixture.run(&["mail", "test", "luiz@example.org"]).await;
        assert_eq!(
            output.unwrap(),
            format!("queued message {}\n", due[0].id + 1)
        );
    }

    #[tokio::test]
    async fn refuses_what_is_not_an_address() {
        let fixture = Fixture::start().await;

        let error = fixture.run(&["mail", "test", "luiz"]).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "That doesn't look like an email address."
        );
        let due = fixture.outbox().await.due(db::now()).await.unwrap();
        assert!(due.is_empty());
    }
}
