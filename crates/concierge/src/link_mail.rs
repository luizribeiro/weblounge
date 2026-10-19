//! Mail that carries the code of a link: sealed to the mailer's key, so
//! the disk never holds the code, and given up on once the code lapses.

use outbox::{Message, NoKey, Outbox};

/// A code just issued, to mail once.
pub trait Issued {
    fn expires_at(&self) -> i64;
}

/// Issues a code with `issue` and queues the email `write` makes of it,
/// sealed; answers the email's id and the code, unless `issue` had none
/// to issue. Fails with [`NoKey`] while the mailer is down, which is
/// checked before issuing, so the member keeps the link they have.
pub async fn send<T: Issued>(
    outbox: &Outbox,
    now: i64,
    issue: impl Future<Output = sqlx::Result<Option<T>>>,
    write: impl FnOnce(&T) -> anyhow::Result<Message>,
) -> anyhow::Result<Option<(i64, T)>> {
    if outbox.published_key().await?.is_none() {
        return Err(NoKey.into());
    }
    let Some(issued) = issue.await? else {
        return Ok(None);
    };
    let id = outbox
        .queue_sealed(&write(&issued)?, now, issued.expires_at())
        .await?;
    Ok(Some((id, issued)))
}
