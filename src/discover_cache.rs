//! Single-flight + optional-TTL coordinator for upstream discovery.
//!
//! `list_tools` / `list_resources` / `list_prompts` each call
//! `RoxyServer::discover`, which performs a full upstream round-trip. An MCP
//! client typically issues all three at connection start, so a single
//! handshake fans out into three back-to-back discovery calls. This
//! coordinator removes the redundant work without changing the default
//! "always fresh" contract:
//!
//! 1. **Single-flight (always on).** Concurrent discovers are coalesced into
//!    one upstream call — the first caller fetches while later callers block,
//!    then everyone shares the one result. Freshness is preserved: a served
//!    result always reflects a fetch that *completed at or after* the caller
//!    began, so no caller ever sees data older than its own request.
//!
//! 2. **TTL cache (opt-in, default off).** When `ttl > 0`, a completed fetch
//!    is also reused by any caller arriving within `ttl` of it — including
//!    sequential back-to-back calls. `ttl == 0` disables this entirely,
//!    leaving only single-flight active, which is the default behaviour.
//!
//! The staleness window is therefore explicit: `0` by default (no time-based
//! staleness; concurrent callers only ever share an in-flight fetch), and at
//! most `ttl` once opted in.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// Coordinator holding the last successful discovery and serializing fetches.
pub struct DiscoverCache<T> {
    /// `Duration::ZERO` disables time-based caching (single-flight only).
    ttl: Duration,
    /// `None` until the first successful fetch. Guarded by an async `Mutex`
    /// whose lock is deliberately held across the upstream call so concurrent
    /// callers coalesce onto one fetch instead of each issuing their own.
    state: Mutex<Option<Cached<T>>>,
}

struct Cached<T> {
    /// Monotonic instant at which the stored fetch *completed*.
    at: Instant,
    /// Shared so the leader can stash and return the same value without an
    /// extra clone for itself; callers clone out of the `Arc`.
    value: Arc<T>,
}

impl<T: Clone> DiscoverCache<T> {
    /// Create a coordinator. `ttl == Duration::ZERO` keeps the default
    /// always-fresh behaviour (single-flight coalescing only).
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            state: Mutex::new(None),
        }
    }

    /// Return a cached discovery if one is valid for this caller, otherwise
    /// run `fetch`, store its result, and return it.
    ///
    /// "Valid" means either: the stored fetch is within the TTL window
    /// (time-based cache), or it completed at/after this caller entered
    /// (the caller waited on an in-flight fetch — single-flight). The lock is
    /// held across `fetch().await` on purpose: that is what makes overlapping
    /// callers share one upstream round-trip. The flip side is head-of-line
    /// blocking — a slow `fetch` stalls every waiter — so callers must keep
    /// `fetch` bounded (roxy's upstream discovery is capped by
    /// `--upstream-timeout`, so the lock is held for at most that long). A
    /// failed `fetch` is **not** cached — the lock is released and the next
    /// caller retries — so a transient upstream error cannot poison the cache
    /// for the whole TTL.
    pub async fn get_or_fetch<F, Fut, E>(&self, fetch: F) -> Result<T, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        // Captured before contending for the lock: this is "when my request
        // began", the reference point for single-flight freshness.
        let entered = Instant::now();
        let mut guard = self.state.lock().await;

        if let Some(cached) = guard.as_ref() {
            let fresh_by_ttl = !self.ttl.is_zero() && cached.at.elapsed() < self.ttl;
            // `>=` so a caller that entered at the exact instant the fetch
            // completed still coalesces rather than re-fetching.
            let fresh_by_single_flight = cached.at >= entered;
            if fresh_by_ttl || fresh_by_single_flight {
                return Ok((*cached.value).clone());
            }
        }

        let value = Arc::new(fetch().await?);
        *guard = Some(Cached {
            at: Instant::now(),
            value: Arc::clone(&value),
        });
        Ok((*value).clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    /// With the TTL disabled, concurrent callers that overlap an in-flight
    /// fetch are coalesced into a single upstream call and all receive the
    /// leader's value — the core single-flight guarantee, active by default.
    #[tokio::test]
    async fn single_flight_coalesces_concurrent_calls_with_ttl_zero() {
        const FOLLOWERS: usize = 5;
        let cache: Arc<DiscoverCache<u64>> = Arc::new(DiscoverCache::new(Duration::ZERO));
        let calls = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Notify::new());
        let can_finish = Arc::new(Notify::new());

        // Leader: enters the fetch (taking the lock), announces it has started,
        // then parks until the test releases it — guaranteeing its fetch is
        // still in flight while the followers enter.
        let leader = {
            let (cache, calls) = (Arc::clone(&cache), Arc::clone(&calls));
            let (started, can_finish) = (Arc::clone(&started), Arc::clone(&can_finish));
            tokio::spawn(async move {
                cache
                    .get_or_fetch(|| async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        started.notify_one();
                        can_finish.notified().await;
                        Ok::<u64, ()>(42)
                    })
                    .await
            })
        };

        // Block until the leader is provably inside the fetch holding the lock.
        started.notified().await;

        // Followers enter while the leader's fetch is in flight; each would
        // return 99 if it ran its own fetch. Each bumps `ready` immediately
        // before calling `get_or_fetch`; because nothing awaits between that
        // bump and the `entered` timestamp captured inside `get_or_fetch`,
        // `ready == FOLLOWERS` proves every follower captured `entered` (and is
        // now parked on the lock) before we release the leader — no wall-clock
        // sleep, so the test cannot flake under scheduler jitter.
        let mut followers = Vec::new();
        for _ in 0..FOLLOWERS {
            let (cache, calls, ready) =
                (Arc::clone(&cache), Arc::clone(&calls), Arc::clone(&ready));
            followers.push(tokio::spawn(async move {
                ready.fetch_add(1, Ordering::SeqCst);
                cache
                    .get_or_fetch(|| async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok::<u64, ()>(99)
                    })
                    .await
            }));
        }

        // Deterministically wait until all followers have entered and parked.
        while ready.load(Ordering::SeqCst) < FOLLOWERS {
            tokio::task::yield_now().await;
        }
        can_finish.notify_one();

        assert_eq!(leader.await.unwrap(), Ok(42));
        for f in followers {
            assert_eq!(
                f.await.unwrap(),
                Ok(42),
                "followers must receive the coalesced leader value, not their own"
            );
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "concurrent discovers must collapse into one upstream call"
        );
    }

    /// With the TTL disabled, sequential (non-overlapping) calls each perform
    /// their own fetch — the default "always fresh" behaviour is preserved.
    #[tokio::test]
    async fn ttl_zero_does_not_cache_sequential_calls() {
        let cache: DiscoverCache<u64> = DiscoverCache::new(Duration::ZERO);
        let calls = AtomicUsize::new(0);

        for _ in 0..3 {
            let v = cache
                .get_or_fetch(|| async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<u64, ()>(7)
                })
                .await;
            assert_eq!(v, Ok(7));
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "no TTL means each sequential discover is a fresh upstream call"
        );
    }

    /// With a TTL set, sequential calls within the window are served from
    /// cache; once the window elapses a fresh fetch runs.
    #[tokio::test]
    async fn ttl_caches_sequential_calls_until_expiry() {
        let cache: DiscoverCache<u64> = DiscoverCache::new(Duration::from_millis(50));
        let calls = AtomicUsize::new(0);
        let fetch = |c: &AtomicUsize| {
            c.fetch_add(1, Ordering::SeqCst);
        };

        assert_eq!(
            cache
                .get_or_fetch(|| async {
                    fetch(&calls);
                    Ok::<u64, ()>(1)
                })
                .await,
            Ok(1)
        );
        // Second call is well within the TTL → served from cache.
        assert_eq!(
            cache
                .get_or_fetch(|| async {
                    fetch(&calls);
                    Ok::<u64, ()>(1)
                })
                .await,
            Ok(1)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "within TTL: cached");

        // Wait out the TTL, then the next call must re-fetch.
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(
            cache
                .get_or_fetch(|| async {
                    fetch(&calls);
                    Ok::<u64, ()>(1)
                })
                .await,
            Ok(1)
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "after TTL expiry a fresh upstream call runs"
        );
    }

    /// A failed fetch is not stored, so the next caller retries instead of
    /// being served a cached error for the whole TTL.
    #[tokio::test]
    async fn fetch_error_is_not_cached() {
        // Long TTL to prove the *error*, not time, is what isn't cached.
        let cache: DiscoverCache<u64> = DiscoverCache::new(Duration::from_secs(3600));

        let first: Result<u64, &str> = cache.get_or_fetch(|| async { Err("upstream down") }).await;
        assert_eq!(first, Err("upstream down"));

        let second: Result<u64, &str> = cache.get_or_fetch(|| async { Ok(5) }).await;
        assert_eq!(
            second,
            Ok(5),
            "a prior error must not poison the cache; the retry fetches fresh"
        );
    }
}
