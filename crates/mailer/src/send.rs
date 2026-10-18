//! concierge-mailer: sends what concierge leaves in the outbox, deleting
//! each message once it's sent or can never be. It logs message ids,
//! counts and SMTP reply codes only, never an address, a subject or a
//! body.

use std::{
    fmt,
    future::Future,
    time::{Duration, Instant},
};

use common::{email::Email, now, random, recent::Quota};
use lettre::{
    Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    message::{Mailbox, SinglePart},
    transport::smtp::{
        self,
        authentication::Credentials,
        response::{Category, Code, Severity},
    },
};
use outbox::{Body, Deletion, Outbox, QueuedMessage, Subject};

use crate::{config::MailerConfig, key::Key};

/// How long the outbox sits unchecked. Concierge can't wake the mailer, and
/// a member waiting for a verification email notices more than a few
/// seconds.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// MXroute takes 400 messages an hour per account; staying well under
/// leaves room for mail sent from the mailbox by hand.
pub const SENDS_PER_HOUR: usize = 300;
pub const HOUR: Duration = Duration::from_secs(60 * 60);
const MESSAGE_ID_LENGTH: usize = 22;

/// Why a message wasn't sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The server's negative reply. Its text is left out, as it often
    /// quotes the recipient.
    Reply(Code),
    /// Anything else, described without the server's words.
    Other(String),
}

impl Failure {
    /// Whether the server refused the message for good.
    pub fn is_permanent(&self) -> bool {
        // 53x replies refuse the login (RFC 4954), which is about our
        // account, not this message.
        matches!(self, Self::Reply(code)
            if code.severity == Severity::PermanentNegativeCompletion
                && code.category != Category::Unspecified3)
    }
}

impl From<&smtp::Error> for Failure {
    fn from(error: &smtp::Error) -> Self {
        match error.status() {
            Some(code) => Self::Reply(code),
            None if error.is_response() => Self::Other("unreadable reply".into()),
            None => Self::Other(error.to_string()),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reply(code) => write!(f, "SMTP {code}"),
            Self::Other(description) => f.write_str(description),
        }
    }
}

/// Where messages go: the SMTP server, or a fake one in tests.
pub trait Transport {
    fn send(&self, message: lettre::Message) -> impl Future<Output = Result<(), Failure>>;
}

impl Transport for AsyncSmtpTransport<Tokio1Executor> {
    async fn send(&self, message: lettre::Message) -> Result<(), Failure> {
        AsyncTransport::send(self, message)
            .await
            .map(drop)
            .map_err(|error| Failure::from(&error))
    }
}

/// The configured server over implicit TLS, its certificate checked
/// against the system's trust store.
pub fn smtp(config: &MailerConfig) -> anyhow::Result<AsyncSmtpTransport<Tokio1Executor>> {
    let credentials = Credentials::new(config.smtp_username.clone(), config.read_smtp_password()?);
    Ok(
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)?
            .port(config.smtp_port)
            .credentials(credentials)
            .build(),
    )
}

/// Logs in to the configured server and leaves without sending anything,
/// which shows the way out, the certificate and the password all work.
pub async fn check(config: &MailerConfig) -> anyhow::Result<String> {
    let server = format!(
        "{}:{} as {}",
        config.smtp_host, config.smtp_port, config.smtp_username
    );
    match smtp(config)?.test_connection().await {
        Ok(true) => Ok(format!("Logged in to {server}\n")),
        Ok(false) => anyhow::bail!("{server} hung up after the login"),
        Err(error) => anyhow::bail!("Can't log in to {server}: {}", Failure::from(&error)),
    }
}

/// What a round did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Round {
    pub sent: usize,
    /// Put off to try again.
    pub failed: usize,
    /// Refused by the server for good, and deleted.
    pub rejected: usize,
    /// Rows that no longer make a message, deleted.
    pub unreadable: usize,
    pub given_up: u64,
    /// Sealed to an earlier mailer's key, and deleted unsent.
    pub sealed_to_others: u64,
    /// Due, but over the hourly quota.
    pub waiting: usize,
}

pub struct Mailer<T> {
    pub outbox: Outbox,
    pub transport: T,
    pub from: Mailbox,
    pub quota: Quota,
    pub key: Key,
}

impl<T: Transport> Mailer<T> {
    /// A mailer with a key of its own, which it has yet to publish.
    pub fn new(outbox: Outbox, transport: T, from: Mailbox) -> Self {
        Self {
            outbox,
            transport,
            from,
            quota: Quota::new(SENDS_PER_HOUR, HOUR),
            key: Key::generate(),
        }
    }

    /// Makes this mailer's key the one concierge seals mail to, deleting
    /// what was sealed to another.
    pub async fn publish_key(&self) -> anyhow::Result<Deletion> {
        let deletion = self.outbox.publish_key(&self.key.public()).await?;
        log_sealed_to_others(deletion.messages);
        Ok(deletion)
    }

    /// Sends the outbox, round after round, until the outbox fails.
    pub async fn run(&self) -> anyhow::Result<()> {
        loop {
            self.round(now(), Instant::now()).await?;
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Deletes the messages given up on and those no key of this mailer's
    /// opens, then tries each one due at `now` while the quota lasts.
    pub async fn round(&self, now: i64, instant: Instant) -> anyhow::Result<Round> {
        let mut round = Round {
            given_up: self.outbox.delete_given_up(now).await?.messages,
            sealed_to_others: (self.outbox)
                .delete_sealed_to_others(&self.key.public())
                .await?
                .messages,
            ..Round::default()
        };
        if round.given_up > 0 {
            eprintln!("Gave up on {} messages and deleted them", round.given_up);
        }
        log_sealed_to_others(round.sealed_to_others);
        let due = self.outbox.due(now).await?;
        for (index, queued) in due.iter().enumerate() {
            let id = queued.id;
            let Some(message) = self.message(queued) else {
                eprintln!("Message {id} doesn't make a message any more; deleting it");
                self.outbox.delete(id).await?;
                round.unreadable += 1;
                continue;
            };
            if !self.quota.take(instant) {
                round.waiting = due.len() - index;
                eprintln!(
                    "Sent {SENDS_PER_HOUR} messages in the last hour; {} wait",
                    round.waiting
                );
                break;
            }
            match self.transport.send(message).await {
                Ok(()) => {
                    self.outbox.delete(id).await?;
                    round.sent += 1;
                    eprintln!("Sent message {id}");
                }
                Err(failure) if failure.is_permanent() => {
                    self.outbox.delete(id).await?;
                    round.rejected += 1;
                    eprintln!("The server refused message {id} ({failure}); deleting it");
                }
                Err(failure) => {
                    self.outbox.record_failed_attempt(queued, now).await?;
                    round.failed += 1;
                    eprintln!(
                        "Sending message {id} failed ({failure}) on attempt {}",
                        queued.attempts + 1
                    );
                }
            }
        }
        Ok(round)
    }

    /// The message `queued` makes, if its recipient and subject still
    /// parse, and its body is plain or opens with this mailer's key.
    fn message(&self, queued: &QueuedMessage) -> Option<lettre::Message> {
        let body = match &queued.body {
            Body::Plain(text) => text.clone(),
            Body::Sealed { ciphertext, .. } => self.key.open(ciphertext)?,
        };
        let to: Address = Email::parse(&queued.recipient)
            .ok()?
            .as_str()
            .parse()
            .ok()?;
        let subject = Subject::parse(&queued.subject).ok()?;
        lettre::Message::builder()
            .message_id(Some(self.message_id()))
            .from(self.from.clone())
            .to(Mailbox::new(None, to))
            .subject(subject.as_str())
            .singlepart(SinglePart::plain(body))
            .ok()
    }

    /// A Message-ID in the sender's domain; lettre's default would name
    /// this host, which isn't a domain anyone can look up.
    fn message_id(&self) -> String {
        let unique = random::string(random::URL_SAFE, MESSAGE_ID_LENGTH);
        format!("<{unique}@{}>", self.from.email.domain())
    }
}

fn log_sealed_to_others(messages: u64) {
    if messages > 0 {
        eprintln!("Deleted {messages} messages sealed to an earlier mailer's key, unsent");
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use lettre::transport::smtp::response::Detail;

    use super::*;
    use crate::config::SMTP_PASSWORD_CREDENTIAL;
    use outbox::{
        GIVE_UP_AFTER,
        testing::{QUEUED, TestKey, VERIFY_SUBJECT, execute, message, with_outbox},
    };

    /// An SMTP server that takes everything but what it's told to refuse.
    #[derive(Default)]
    struct FakeServer {
        refusals: HashMap<&'static str, Failure>,
        delivered: Mutex<Vec<lettre::Message>>,
    }

    impl FakeServer {
        fn refusing(recipient: &'static str, failure: Failure) -> Self {
            Self {
                refusals: HashMap::from([(recipient, failure)]),
                ..Self::default()
            }
        }

        fn recipients(&self) -> Vec<String> {
            let delivered = self.delivered.lock().unwrap();
            delivered.iter().map(recipient).collect()
        }
    }

    fn recipient(message: &lettre::Message) -> String {
        message.envelope().to()[0].to_string()
    }

    impl Transport for FakeServer {
        async fn send(&self, message: lettre::Message) -> Result<(), Failure> {
            if let Some(failure) = self.refusals.get(recipient(&message).as_str()) {
                return Err(failure.clone());
            }
            self.delivered.lock().unwrap().push(message);
            Ok(())
        }
    }

    /// A negative reply such as `reply(5, 5, 0)` for 550.
    fn reply(severity: u8, category: u8, detail: u8) -> Failure {
        let severity = match severity {
            4 => Severity::TransientNegativeCompletion,
            _ => Severity::PermanentNegativeCompletion,
        };
        let category = match category {
            2 => Category::Connections,
            3 => Category::Unspecified3,
            _ => Category::MailSystem,
        };
        let detail = match detail {
            0 => Detail::Zero,
            1 => Detail::One,
            4 => Detail::Four,
            _ => Detail::Five,
        };
        Failure::Reply(Code::new(severity, category, detail))
    }

    /// A mailer with verification emails queued for alice and bob, a
    /// minute apart.
    async fn mailer(server: FakeServer) -> (Mailer<FakeServer>, tempfile::TempDir) {
        let (outbox, dir) = with_outbox().await;
        let from = "weblounge <noreply@weblounge.org>".parse().unwrap();
        (Mailer::new(outbox, server, from), dir)
    }

    /// The recipient and failed attempts of each message due at `now`.
    async fn due(mailer: &Mailer<FakeServer>, now: i64) -> Vec<(String, i64)> {
        let due = mailer.outbox.due(now).await.unwrap();
        due.into_iter()
            .map(|message| (message.recipient, message.attempts))
            .collect()
    }

    fn bobs(attempts: i64) -> (String, i64) {
        ("bob@example.org".into(), attempts)
    }

    fn alices(attempts: i64) -> (String, i64) {
        ("alice@example.org".into(), attempts)
    }

    #[tokio::test]
    async fn publishes_its_own_key() {
        let (mailer, _dir) = mailer(FakeServer::default()).await;

        mailer.publish_key().await.unwrap();

        let published = mailer.outbox.published_key().await.unwrap();
        assert_eq!(published, Some(mailer.key.public()));
    }

    /// Queues carol's verification email, sealed to the key published.
    async fn queue_carols(mailer: &Mailer<FakeServer>) {
        let carols = message(Some("carol"), "carol@example.org", VERIFY_SUBJECT);
        mailer.outbox.queue_sealed(&carols, QUEUED).await.unwrap();
    }

    #[tokio::test]
    async fn opens_and_sends_what_was_sealed_to_its_key() {
        let (mailer, _dir) = mailer(FakeServer::default()).await;
        mailer.publish_key().await.unwrap();
        queue_carols(&mailer).await;

        let round = mailer.round(QUEUED + 60, Instant::now()).await.unwrap();

        assert_eq!((round.sent, round.unreadable), (3, 0));
        let delivered = mailer.transport.delivered.lock().unwrap();
        assert_eq!(recipient(&delivered[1]), "carol@example.org");
        let formatted = String::from_utf8(delivered[1].formatted()).unwrap();
        assert!(
            formatted.contains("verify#q7Hx2mZp9vLcT4wRfBa"),
            "{formatted}"
        );
    }

    #[tokio::test]
    async fn deletes_what_was_sealed_to_an_earlier_mailer_unsent() {
        let (mailer, _dir) = mailer(FakeServer::default()).await;
        TestKey::publish(&mailer.outbox).await;
        queue_carols(&mailer).await;

        let round = mailer.round(QUEUED + 60, Instant::now()).await.unwrap();

        assert_eq!((round.sealed_to_others, round.sent), (1, 2));
        assert_eq!(
            mailer.transport.recipients(),
            ["alice@example.org", "bob@example.org"]
        );
        TestKey::publish(&mailer.outbox).await;
        queue_carols(&mailer).await;
        assert_eq!(mailer.publish_key().await.unwrap().messages, 1);
        assert!(due(&mailer, QUEUED + 60).await.is_empty());
    }

    #[tokio::test]
    async fn sends_each_due_message_and_deletes_it() {
        let (mailer, _dir) = mailer(FakeServer::default()).await;

        let round = mailer.round(QUEUED + 60, Instant::now()).await.unwrap();

        assert_eq!(
            round,
            Round {
                sent: 2,
                ..Round::default()
            }
        );
        assert_eq!(
            mailer.transport.recipients(),
            ["alice@example.org", "bob@example.org"]
        );
        assert!(due(&mailer, QUEUED + 60).await.is_empty());
        let round = mailer.round(QUEUED + 60, Instant::now()).await.unwrap();
        assert_eq!(round, Round::default());
    }

    #[tokio::test]
    async fn builds_a_plain_text_utf8_message_from_the_sender() {
        let (mailer, _dir) = mailer(FakeServer::default()).await;
        mailer.round(QUEUED, Instant::now()).await.unwrap();

        let delivered = mailer.transport.delivered.lock().unwrap();
        let formatted = String::from_utf8(delivered[0].formatted()).unwrap();
        let (headers, body) = formatted.split_once("\r\n\r\n").unwrap();
        let header = |name: &str| {
            let prefix = format!("{name}: ");
            headers
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap_or_else(|| panic!("no {name} in {headers}"))
                .to_owned()
        };
        assert_eq!(header("From"), "weblounge <noreply@weblounge.org>");
        assert_eq!(header("To"), "alice@example.org");
        assert_eq!(header("Subject"), VERIFY_SUBJECT);
        assert_eq!(header("Content-Type"), "text/plain; charset=utf-8");
        assert_eq!(header("MIME-Version"), "1.0");
        assert!(header("Date").ends_with(" +0000"), "{headers}");
        let message_id = regex::Regex::new(r"^<[A-Za-z0-9_-]{22}@weblounge\.org>$").unwrap();
        assert!(message_id.is_match(&header("Message-ID")), "{headers}");
        assert!(body.contains("verify#q7Hx2mZp9vLcT4wRfBa"), "{body}");
        assert!(body.contains("Zo=C3=AB says hello"), "{body}");
    }

    #[tokio::test]
    async fn puts_a_message_off_when_sending_it_fails_for_now() {
        let server = FakeServer::refusing("alice@example.org", reply(4, 2, 1));
        let (mailer, _dir) = mailer(server).await;

        let round = mailer.round(QUEUED + 60, Instant::now()).await.unwrap();

        assert_eq!((round.sent, round.failed), (1, 1));
        assert_eq!(mailer.transport.recipients(), ["bob@example.org"]);
        let retry_at = outbox::next_attempt_at(1, QUEUED + 60);
        assert!(due(&mailer, retry_at - 1).await.is_empty());
        assert_eq!(due(&mailer, retry_at).await, [alices(1)]);
    }

    #[tokio::test]
    async fn deletes_a_message_the_server_refuses_for_good() {
        let server = FakeServer::refusing("alice@example.org", reply(5, 5, 0));
        let (mailer, _dir) = mailer(server).await;

        let round = mailer.round(QUEUED, Instant::now()).await.unwrap();

        assert_eq!((round.rejected, round.failed), (1, 0));
        assert_eq!(due(&mailer, QUEUED + 60).await, [bobs(0)]);
    }

    #[tokio::test]
    async fn tries_again_after_the_server_refuses_the_login() {
        let server = FakeServer::refusing("alice@example.org", reply(5, 3, 5));
        let (mailer, _dir) = mailer(server).await;

        let round = mailer.round(QUEUED, Instant::now()).await.unwrap();

        assert_eq!((round.rejected, round.failed), (0, 1));
        let retry_at = outbox::next_attempt_at(1, QUEUED);
        assert_eq!(due(&mailer, retry_at).await, [alices(1), bobs(0)]);
    }

    #[tokio::test]
    async fn deletes_what_it_gave_up_on_without_sending_it() {
        let (mailer, _dir) = mailer(FakeServer::default()).await;
        let alices_end = QUEUED + GIVE_UP_AFTER + 1;

        let round = mailer.round(alices_end, Instant::now()).await.unwrap();

        assert_eq!((round.given_up, round.sent), (1, 1));
        assert_eq!(mailer.transport.recipients(), ["bob@example.org"]);
        assert!(due(&mailer, QUEUED).await.is_empty());
    }

    #[tokio::test]
    async fn messages_over_the_hourly_quota_wait_for_it() {
        let (mut mailer, _dir) = mailer(FakeServer::default()).await;
        mailer.quota = Quota::new(1, HOUR);
        let start = Instant::now();
        let second = Duration::from_secs(1);

        let round = mailer.round(QUEUED + 60, start).await.unwrap();
        assert_eq!((round.sent, round.waiting), (1, 1));
        let round = mailer.round(QUEUED + 3599, start + HOUR - second);
        assert_eq!(round.await.unwrap().waiting, 1);
        let round = mailer.round(QUEUED + 3600, start + HOUR).await.unwrap();
        assert_eq!((round.sent, round.waiting), (1, 0));

        assert_eq!(
            mailer.transport.recipients(),
            ["alice@example.org", "bob@example.org"]
        );
    }

    #[tokio::test]
    async fn deletes_rows_that_no_longer_make_a_message() {
        let (mailer, _dir) = mailer(FakeServer::default()).await;
        let key = mailer.key.public();
        execute(
            &mailer.outbox,
            &format!(
                "INSERT INTO outbox (recipient, subject, body, created_at, next_attempt_at)
                 VALUES ('carol', 'Verify', 'Hi', 1760000000, 1760000000),
                        ('dave@example.org', 'Verify' || char(10) || 'Bcc: x', 'Hi',
                         1760000000, 1760000000);
                 INSERT INTO outbox (recipient, subject, sealed_to, sealed_body, created_at,
                                     next_attempt_at)
                 VALUES ('erin@example.org', 'Verify', '{key}', x'00', 1760000000, 1760000000);"
            ),
        )
        .await
        .unwrap();

        let round = mailer.round(QUEUED + 60, Instant::now()).await.unwrap();

        assert_eq!((round.unreadable, round.sent), (3, 2));
        assert!(due(&mailer, QUEUED + 60).await.is_empty());
    }

    #[test]
    fn only_a_refusal_of_the_message_is_permanent() {
        assert!(reply(5, 5, 0).is_permanent());
        assert!(reply(5, 5, 4).is_permanent());
        assert!(!reply(5, 3, 5).is_permanent());
        assert!(!reply(4, 2, 1).is_permanent());
        assert!(!Failure::Other("Connection error: refused".into()).is_permanent());
        assert_eq!(reply(5, 5, 0).to_string(), "SMTP 550");
    }

    /// The mailer's config for an SMTP server at `port` on loopback, with
    /// the password in `credentials`.
    fn config_for(port: u16, credentials: &tempfile::TempDir) -> MailerConfig {
        let password = credentials.path().join(SMTP_PASSWORD_CREDENTIAL);
        std::fs::write(password, "correct horse battery staple\n").unwrap();
        MailerConfig {
            smtp_host: "127.0.0.1".into(),
            smtp_port: port,
            smtp_username: "noreply@weblounge.org".into(),
            from: "noreply@weblounge.org".parse().unwrap(),
            outbox_path: "/var/lib/concierge-outbox/outbox.db".into(),
            credentials_dir: credentials.path().into(),
        }
    }

    /// A loopback port nothing listens on.
    fn closed_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    /// Sends a message through the SMTP transport to `port` on loopback.
    async fn send_to(port: u16) -> Failure {
        let credentials = tempfile::tempdir().unwrap();
        let config = config_for(port, &credentials);
        let message = lettre::Message::builder()
            .from(config.from.clone())
            .to("alice@example.org".parse().unwrap())
            .body("Hi alice".to_owned())
            .unwrap();
        let server = smtp(&config).unwrap();
        Transport::send(&server, message).await.unwrap_err()
    }

    #[tokio::test]
    async fn an_unreachable_server_fails_for_now() {
        let failure = send_to(closed_port()).await;

        assert!(matches!(failure, Failure::Other(_)), "{failure:?}");
        assert!(!failure.is_permanent());
    }

    #[tokio::test]
    async fn checking_an_unreachable_server_names_it_and_the_login() {
        let credentials = tempfile::tempdir().unwrap();
        let port = closed_port();

        let error = check(&config_for(port, &credentials)).await.unwrap_err();

        let expected = format!("Can't log in to 127.0.0.1:{port} as noreply@weblounge.org: ");
        let error = error.to_string();
        assert!(error.starts_with(&expected), "{error}");
        assert!(error.contains("Connection refused"), "{error}");
    }

    #[tokio::test]
    async fn never_talks_to_a_server_without_tls() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let plaintext = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let greeting = b"220 mail.weblounge.localhost ESMTP\r\n";
            tokio::io::AsyncWriteExt::write_all(&mut connection, greeting).await
        });

        let failure = send_to(port).await;

        assert!(failure.to_string().contains("corrupt message"), "{failure}");
        assert!(!failure.is_permanent());
        plaintext.await.unwrap().ok();
    }
}
