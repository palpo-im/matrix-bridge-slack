use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

struct TimedValue<V> {
    value: V,
    inserted_at: Instant,
}

pub struct TimedCache<K, V> {
    map: HashMap<K, TimedValue<V>>,
    ttl: Duration,
}

impl<K, V> TimedCache<K, V>
where
    K: std::hash::Hash + Eq + Clone,
{
    pub fn new(ttl: Duration) -> Self {
        Self {
            map: HashMap::new(),
            ttl,
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key).and_then(|tv| {
            if tv.inserted_at.elapsed() < self.ttl {
                Some(&tv.value)
            } else {
                None
            }
        })
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.map.insert(
            key,
            TimedValue {
                value,
                inserted_at: Instant::now(),
            },
        );
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.map.remove(key).map(|tv| tv.value)
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    pub fn cleanup_expired(&mut self) {
        self.map.retain(|_, tv| tv.inserted_at.elapsed() < self.ttl);
    }
}

pub struct AsyncTimedCache<K, V> {
    inner: RwLock<TimedCache<K, V>>,
}

impl<K, V> AsyncTimedCache<K, V>
where
    K: std::hash::Hash + Eq + Clone,
{
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: RwLock::new(TimedCache::new(ttl)),
        }
    }

    pub async fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.inner.read().await.get(key).cloned()
    }

    pub async fn insert(&self, key: K, value: V) {
        self.inner.write().await.insert(key, value);
    }

    pub async fn remove(&self, key: &K) -> Option<V> {
        self.inner.write().await.remove(key)
    }

    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }

    pub async fn cleanup_expired(&self) {
        self.inner.write().await.cleanup_expired();
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;

    use super::*;

    #[test]
    fn timed_cache_returns_value_before_expiry() {
        let mut cache: TimedCache<&str, &str> = TimedCache::new(Duration::from_millis(100));
        cache.insert("key", "value");
        assert_eq!(cache.get(&"key"), Some(&"value"));
    }

    #[test]
    fn timed_cache_returns_none_after_expiry() {
        let mut cache: TimedCache<&str, &str> = TimedCache::new(Duration::from_millis(50));
        cache.insert("key", "value");
        sleep(Duration::from_millis(60));
        assert_eq!(cache.get(&"key"), None);
    }

    #[test]
    fn timed_cache_remove_deletes_entry() {
        let mut cache: TimedCache<&str, &str> = TimedCache::new(Duration::from_secs(10));
        cache.insert("key", "value");
        assert_eq!(cache.remove(&"key"), Some("value"));
        assert_eq!(cache.get(&"key"), None);
    }

    #[test]
    fn timed_cache_cleanup_removes_expired() {
        let mut cache: TimedCache<&str, &str> = TimedCache::new(Duration::from_millis(50));
        cache.insert("key1", "value1");
        cache.insert("key2", "value2");
        sleep(Duration::from_millis(60));
        cache.insert("key3", "value3");
        cache.cleanup_expired();
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&"key3"), Some(&"value3"));
    }

    #[tokio::test]
    async fn async_timed_cache_returns_value_before_expiry() {
        let cache: AsyncTimedCache<&str, &str> = AsyncTimedCache::new(Duration::from_millis(100));
        cache.insert("key", "value").await;
        assert_eq!(cache.get(&"key").await, Some("value"));
    }

    #[tokio::test]
    async fn async_timed_cache_returns_none_after_expiry() {
        let cache: AsyncTimedCache<&str, &str> = AsyncTimedCache::new(Duration::from_millis(50));
        cache.insert("key", "value").await;
        sleep(Duration::from_millis(60));
        assert_eq!(cache.get(&"key").await, None);
    }
}
