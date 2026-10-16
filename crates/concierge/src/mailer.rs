//! concierge-mailer: sends what concierge leaves in the outbox. It logs
//! message ids, counts and SMTP reply codes only, never an address, a
//! subject or a body.

use std::{fmt, future::Future};

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    transport::smtp::{
        self,
        authentication::Credentials,
        response::{Category, Code, Severity},
    },
};

use crate::config::MailerConfig;

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

#[cfg(test)]
mod tests {
    use lettre::transport::smtp::response::Detail;

    use super::*;
    use crate::config::SMTP_PASSWORD_CREDENTIAL;

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

    #[test]
    fn only_a_refusal_of_the_message_is_permanent() {
        assert!(reply(5, 5, 0).is_permanent());
        assert!(reply(5, 5, 4).is_permanent());
        assert!(!reply(5, 3, 5).is_permanent());
        assert!(!reply(4, 2, 1).is_permanent());
        assert!(!Failure::Other("Connection error: refused".into()).is_permanent());
        assert_eq!(reply(5, 5, 0).to_string(), "SMTP 550");
    }

    /// Sends a message through the SMTP transport to `port` on loopback.
    async fn send_to(port: u16) -> Failure {
        let credentials = tempfile::tempdir().unwrap();
        let password = credentials.path().join(SMTP_PASSWORD_CREDENTIAL);
        std::fs::write(password, "correct horse battery staple\n").unwrap();
        let config = MailerConfig {
            smtp_host: "127.0.0.1".into(),
            smtp_port: port,
            smtp_username: "noreply@weblounge.org".into(),
            from: "noreply@weblounge.org".parse().unwrap(),
            state_dir: "/var/lib/concierge".into(),
            credentials_dir: credentials.path().into(),
        };
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
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let failure = send_to(port).await;

        assert!(matches!(failure, Failure::Other(_)), "{failure:?}");
        assert!(!failure.is_permanent());
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
