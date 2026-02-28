use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

pub struct ChannelQueue {
    queues: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl ChannelQueue {
    pub fn new() -> Self {
        Self {
            queues: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn enqueue<F, Fut>(&self, channel_id: &str, task: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mutex = {
            let mut queues = self.queues.lock().await;
            queues
                .entry(channel_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };

        tokio::spawn(async move {
            let _guard = mutex.lock().await;
            task().await;
        });
    }

    pub async fn enqueue_fut<F>(&self, channel_id: &str, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let mutex = {
            let mut queues = self.queues.lock().await;
            queues
                .entry(channel_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };

        tokio::spawn(async move {
            let _guard = mutex.lock().await;
            task.await;
        });
    }
}

impl Default for ChannelQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::time::{Duration, sleep};

    use super::*;

    #[tokio::test]
    async fn channel_queue_processes_in_order() {
        let queue = ChannelQueue::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(Mutex::new(Vec::new()));

        let c1 = counter.clone();
        let o1 = order.clone();
        queue
            .enqueue_fut("channel1", async move {
                sleep(Duration::from_millis(50)).await;
                let val = c1.fetch_add(1, Ordering::SeqCst);
                o1.lock().await.push(val);
            })
            .await;

        let c2 = counter.clone();
        let o2 = order.clone();
        queue
            .enqueue_fut("channel1", async move {
                let val = c2.fetch_add(1, Ordering::SeqCst);
                o2.lock().await.push(val);
            })
            .await;

        let c3 = counter.clone();
        let o3 = order.clone();
        queue
            .enqueue_fut("channel1", async move {
                let val = c3.fetch_add(1, Ordering::SeqCst);
                o3.lock().await.push(val);
            })
            .await;

        sleep(Duration::from_millis(200)).await;

        let result = order.lock().await.clone();
        assert_eq!(result, vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn different_channels_process_independently() {
        let queue = ChannelQueue::new();
        let order = Arc::new(Mutex::new(Vec::new()));

        let o1 = order.clone();
        queue
            .enqueue_fut("channel1", async move {
                sleep(Duration::from_millis(50)).await;
                o1.lock().await.push("ch1");
            })
            .await;

        let o2 = order.clone();
        queue
            .enqueue_fut("channel2", async move {
                o2.lock().await.push("ch2");
            })
            .await;

        sleep(Duration::from_millis(100)).await;

        let result = order.lock().await.clone();
        assert_eq!(result, vec!["ch2", "ch1"]);
    }
}
