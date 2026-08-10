use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::ToolRateLimitConfig;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolClass {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FallbackPrincipal {
    Stdio,
    HttpToken,
    HttpNone,
    HttpOAuth,
}

impl FallbackPrincipal {
    pub(crate) const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Stdio => Some("transport:stdio"),
            Self::HttpToken => Some("transport:http-token"),
            Self::HttpNone => Some("transport:http-none"),
            Self::HttpOAuth => None,
        }
    }
}

impl ToolClass {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RateLimitLayer {
    Principal,
    Global,
    PrincipalAndGlobal,
}

impl RateLimitLayer {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Principal => "principal",
            Self::Global => "global",
            Self::PrincipalAndGlobal => "principal_and_global",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RateLimitRejection {
    pub(crate) class: ToolClass,
    pub(crate) layer: RateLimitLayer,
    pub(crate) retry_after_seconds: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RateLimiterUnavailable;

impl fmt::Display for RateLimiterUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("tool rate limiter state is unavailable")
    }
}

impl std::error::Error for RateLimiterUnavailable {}

trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Clone, Copy)]
struct BucketQuota {
    refill_per_second: f64,
    capacity: f64,
}

impl BucketQuota {
    fn new(per_minute: u32, burst: u32) -> Self {
        Self {
            refill_per_second: f64::from(per_minute) / 60.0,
            capacity: f64::from(burst),
        }
    }
}

#[derive(Clone, Copy)]
struct ClassQuota {
    per_principal: BucketQuota,
    global: BucketQuota,
}

#[derive(Clone, Copy)]
struct Quotas {
    read: ClassQuota,
    write: ClassQuota,
}

impl Quotas {
    fn from_config(config: &ToolRateLimitConfig) -> Self {
        Self {
            read: ClassQuota {
                per_principal: BucketQuota::new(
                    config.read.per_principal_per_minute,
                    config.read.per_principal_burst,
                ),
                global: BucketQuota::new(config.read.global_per_minute, config.read.global_burst),
            },
            write: ClassQuota {
                per_principal: BucketQuota::new(
                    config.write.per_principal_per_minute,
                    config.write.per_principal_burst,
                ),
                global: BucketQuota::new(config.write.global_per_minute, config.write.global_burst),
            },
        }
    }

    const fn for_class(self, class: ToolClass) -> ClassQuota {
        match class {
            ToolClass::Read => self.read,
            ToolClass::Write => self.write,
        }
    }
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl Bucket {
    fn full(quota: BucketQuota, now: Instant) -> Self {
        Self {
            tokens: quota.capacity,
            last_refill: now,
        }
    }

    fn available(&self, quota: BucketQuota, now: Instant) -> f64 {
        let elapsed = now
            .checked_duration_since(self.last_refill)
            .unwrap_or(Duration::ZERO);
        (self.tokens + elapsed.as_secs_f64() * quota.refill_per_second).min(quota.capacity)
    }

    fn consume(&mut self, available: f64, now: Instant) {
        self.tokens = available - 1.0;
        self.last_refill = now;
    }
}

struct ClassBuckets {
    read: Bucket,
    write: Bucket,
}

impl ClassBuckets {
    fn full(read: BucketQuota, write: BucketQuota, now: Instant) -> Self {
        Self {
            read: Bucket::full(read, now),
            write: Bucket::full(write, now),
        }
    }

    const fn for_class(&self, class: ToolClass) -> &Bucket {
        match class {
            ToolClass::Read => &self.read,
            ToolClass::Write => &self.write,
        }
    }

    const fn for_class_mut(&mut self, class: ToolClass) -> &mut Bucket {
        match class {
            ToolClass::Read => &mut self.read,
            ToolClass::Write => &mut self.write,
        }
    }
}

struct LimiterState {
    global: ClassBuckets,
    principals: HashMap<String, ClassBuckets>,
}

/// Process-local hierarchical limiter shared by every transport handler.
pub(crate) struct ToolRateLimiter {
    quotas: Quotas,
    clock: Arc<dyn Clock>,
    state: Mutex<LimiterState>,
}

impl ToolRateLimiter {
    pub(crate) fn new(config: &ToolRateLimitConfig) -> anyhow::Result<Arc<Self>> {
        config.validate()?;
        Ok(Arc::new(Self::with_clock(config, Arc::new(SystemClock))))
    }

    fn with_clock(config: &ToolRateLimitConfig, clock: Arc<dyn Clock>) -> Self {
        let quotas = Quotas::from_config(config);
        let now = clock.now();
        Self {
            quotas,
            clock,
            state: Mutex::new(LimiterState {
                global: ClassBuckets::full(quotas.read.global, quotas.write.global, now),
                principals: HashMap::new(),
            }),
        }
    }

    /// Atomically consumes both the Principal and global allowance, or neither.
    pub(crate) fn check(
        &self,
        principal: &str,
        class: ToolClass,
    ) -> Result<Option<RateLimitRejection>, RateLimiterUnavailable> {
        let now = self.clock.now();
        let mut state = self
            .state
            .lock()
            .map_err(|_poisoned| RateLimiterUnavailable)?;
        let quota = self.quotas.for_class(class);

        state
            .principals
            .entry(principal.to_string())
            .or_insert_with(|| {
                ClassBuckets::full(
                    self.quotas.read.per_principal,
                    self.quotas.write.per_principal,
                    now,
                )
            });

        let global_available = state.global.for_class(class).available(quota.global, now);
        let principal_available = state
            .principals
            .get(principal)
            .ok_or(RateLimiterUnavailable)?
            .for_class(class)
            .available(quota.per_principal, now);
        let global_limited = global_available < 1.0;
        let principal_limited = principal_available < 1.0;

        if !global_limited && !principal_limited {
            state
                .global
                .for_class_mut(class)
                .consume(global_available, now);
            state
                .principals
                .get_mut(principal)
                .ok_or(RateLimiterUnavailable)?
                .for_class_mut(class)
                .consume(principal_available, now);
            return Ok(None);
        }

        let layer = match (principal_limited, global_limited) {
            (true, true) => RateLimitLayer::PrincipalAndGlobal,
            (true, false) => RateLimitLayer::Principal,
            (false, true) => RateLimitLayer::Global,
            (false, false) => return Ok(None),
        };
        let principal_wait = wait_for_token(principal_available, quota.per_principal);
        let global_wait = wait_for_token(global_available, quota.global);
        let wait = principal_wait.max(global_wait);
        let retry_after_seconds = wait
            .as_secs()
            .saturating_add(u64::from(wait.subsec_nanos() > 0))
            .max(1);
        Ok(Some(RateLimitRejection {
            class,
            layer,
            retry_after_seconds,
        }))
    }
}

fn wait_for_token(available: f64, quota: BucketQuota) -> Duration {
    if available >= 1.0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64((1.0 - available) / quota.refill_per_second)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{Clock, RateLimitLayer, ToolClass, ToolRateLimiter};
    use crate::config::ToolRateLimitConfig;

    struct ManualClock(Mutex<Instant>);

    impl ManualClock {
        fn new() -> Self {
            Self(Mutex::new(Instant::now()))
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.0.lock().expect("manual clock lock");
            *now = now
                .checked_add(duration)
                .expect("test time remains in range");
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            *self.0.lock().expect("manual clock lock")
        }
    }

    fn limiter_with_clock(
        config: &ToolRateLimitConfig,
    ) -> (Arc<ToolRateLimiter>, Arc<ManualClock>) {
        config.validate().expect("test rate limits are valid");
        let clock = Arc::new(ManualClock::new());
        let limiter = Arc::new(ToolRateLimiter::with_clock(config, clock.clone()));
        (limiter, clock)
    }

    #[test]
    fn burst_exhaustion_refills_continuously() {
        let mut config = ToolRateLimitConfig::default();
        config.read.per_principal_per_minute = 60;
        config.read.per_principal_burst = 2;
        let (limiter, clock) = limiter_with_clock(&config);

        assert_eq!(limiter.check("client-a", ToolClass::Read), Ok(None));
        assert_eq!(limiter.check("client-a", ToolClass::Read), Ok(None));
        let rejection = limiter
            .check("client-a", ToolClass::Read)
            .expect("limiter remains available")
            .expect("third call exceeds burst");
        assert_eq!(rejection.layer, RateLimitLayer::Principal);
        assert_eq!(rejection.retry_after_seconds, 1);

        clock.advance(Duration::from_secs(1));
        assert_eq!(limiter.check("client-a", ToolClass::Read), Ok(None));
    }

    #[test]
    fn identities_and_classes_have_independent_buckets() {
        let mut config = ToolRateLimitConfig::default();
        config.read.per_principal_burst = 1;
        config.write.per_principal_burst = 1;
        let (limiter, _clock) = limiter_with_clock(&config);

        assert_eq!(limiter.check("client-a", ToolClass::Read), Ok(None));
        assert!(
            limiter
                .check("client-a", ToolClass::Read)
                .expect("limiter remains available")
                .is_some()
        );
        assert_eq!(limiter.check("client-b", ToolClass::Read), Ok(None));
        assert_eq!(limiter.check("client-a", ToolClass::Write), Ok(None));
    }

    #[test]
    fn global_rejection_does_not_consume_principal_allowance() {
        let mut config = ToolRateLimitConfig::default();
        config.read.per_principal_per_minute = 1;
        config.read.per_principal_burst = 1;
        config.read.global_per_minute = 60;
        config.read.global_burst = 1;
        let (limiter, clock) = limiter_with_clock(&config);

        assert_eq!(limiter.check("client-a", ToolClass::Read), Ok(None));
        let rejection = limiter
            .check("client-b", ToolClass::Read)
            .expect("limiter remains available")
            .expect("global bucket is empty");
        assert_eq!(rejection.layer, RateLimitLayer::Global);

        clock.advance(Duration::from_secs(1));
        assert_eq!(limiter.check("client-b", ToolClass::Read), Ok(None));
    }

    #[test]
    fn concurrent_checks_consume_one_atomic_allowance() {
        let mut config = ToolRateLimitConfig::default();
        config.read.per_principal_burst = 1;
        config.read.global_burst = 1;
        let (limiter, _clock) = limiter_with_clock(&config);
        let barrier = Arc::new(Barrier::new(8));
        let mut workers = Vec::new();

        for _worker in 0..8 {
            let limiter = limiter.clone();
            let barrier = barrier.clone();
            workers.push(thread::spawn(move || {
                barrier.wait();
                limiter
                    .check("client-a", ToolClass::Read)
                    .expect("limiter remains available")
                    .is_none()
            }));
        }

        let allowed = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker does not panic"))
            .filter(|allowed| *allowed)
            .count();
        assert_eq!(allowed, 1);
    }
}
