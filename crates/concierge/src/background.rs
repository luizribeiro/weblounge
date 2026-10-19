//! Work a request starts and answers without waiting for, so how long the
//! answer takes says nothing about that work.

use std::sync::{Arc, Mutex, MutexGuard};

use tokio::task::JoinHandle;

#[derive(Clone, Default)]
pub struct Background(Arc<Mutex<Vec<JoinHandle<()>>>>);

impl Background {
    pub fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        let mut tasks = self.tasks();
        tasks.retain(|task| !task.is_finished());
        tasks.push(tokio::spawn(task));
    }

    /// Waits until every task spawned so far, and any they spawn, is done.
    pub async fn settled(&self) {
        loop {
            let tasks = std::mem::take(&mut *self.tasks());
            if tasks.is_empty() {
                return;
            }
            for task in tasks {
                if let Err(error) = task.await {
                    eprintln!("A background task failed: {error}");
                }
            }
        }
    }

    fn tasks(&self) -> MutexGuard<'_, Vec<JoinHandle<()>>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn settling_waits_for_every_task_spawned() {
        let background = Background::default();
        let done = Arc::new(AtomicUsize::new(0));
        for _ in 0..3 {
            let done = done.clone();
            background.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                done.fetch_add(1, Ordering::SeqCst);
            });
        }
        assert_eq!(done.load(Ordering::SeqCst), 0);

        background.settled().await;

        assert_eq!(done.load(Ordering::SeqCst), 3);
        background.settled().await;
    }
}
