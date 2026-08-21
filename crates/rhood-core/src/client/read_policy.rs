use crate::config::HttpConfig;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::watch;

pub(super) const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);

type SleepFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

pub(super) trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn wall_now(&self) -> SystemTime;
    fn sleep_until(&self, deadline: Instant) -> SleepFuture<'_>;
}

pub(super) struct TokioClock;

impl Clock for TokioClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wall_now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn sleep_until(&self, deadline: Instant) -> SleepFuture<'_> {
        Box::pin(tokio::time::sleep_until(tokio::time::Instant::from_std(
            deadline,
        )))
    }
}

struct ReservationState {
    next_ticket: u64,
    serving_ticket: u64,
    cancelled: BTreeSet<u64>,
    last_start: Option<Instant>,
    cooldown_until: Option<Instant>,
}

pub(super) struct ReadTransportPolicy {
    request_interval: Duration,
    max_attempts: u32,
    retry_backoff: Duration,
    clock: Arc<dyn Clock>,
    reservations: Mutex<ReservationState>,
    changed: watch::Sender<u64>,
}

#[derive(Debug)]
pub(super) enum ReadAttemptError<E> {
    Connection(E),
    Timeout(E),
    RateLimited { retry_after_secs: Option<u64> },
    Other(E),
}

impl ReadTransportPolicy {
    pub(super) fn new(config: &HttpConfig, clock: Arc<dyn Clock>) -> Self {
        let (changed, _) = watch::channel(0);
        Self {
            request_interval: Duration::from_millis(config.read_request_interval_ms),
            max_attempts: config.read_max_attempts,
            retry_backoff: Duration::from_millis(config.read_retry_backoff_ms),
            clock,
            reservations: Mutex::new(ReservationState {
                next_ticket: 0,
                serving_ticket: 0,
                cancelled: BTreeSet::new(),
                last_start: None,
                cooldown_until: None,
            }),
            changed,
        }
    }

    pub(super) fn retry_after_delay_secs(&self, value: &str) -> Option<u64> {
        let value = value.trim();
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            let seconds = value.bytes().fold(0_u64, |seconds, byte| {
                seconds
                    .saturating_mul(10)
                    .saturating_add(u64::from(byte.saturating_sub(b'0')))
            });
            return Some(seconds);
        }

        let retry_at = parse_http_date(value)?;
        let wall_now: DateTime<Utc> = self.clock.wall_now().into();
        let delay = retry_at.signed_duration_since(wall_now);
        if delay <= chrono::TimeDelta::zero() {
            return Some(0);
        }
        let delay = delay.to_std().ok()?;
        let seconds = delay.as_secs();
        if delay.subsec_nanos() == 0 {
            Some(seconds)
        } else {
            seconds.checked_add(1)
        }
    }

    pub(super) async fn execute<T, E, F, Fut>(
        &self,
        mut send: F,
    ) -> std::result::Result<T, ReadAttemptError<E>>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = std::result::Result<T, ReadAttemptError<E>>>,
    {
        let mut attempt = 1;
        loop {
            self.wait_for_start().await;
            match send().await {
                Ok(value) => return Ok(value),
                Err(error @ (ReadAttemptError::Connection(_) | ReadAttemptError::Timeout(_))) => {
                    if attempt == self.max_attempts {
                        return Err(error);
                    }
                    self.wait_for_retry(self.retry_delay(attempt)).await;
                }
                Err(ReadAttemptError::RateLimited { retry_after_secs }) => {
                    let reported_delay = retry_after_secs.unwrap_or(60);
                    if retry_after_secs.is_some_and(|seconds| seconds > 60) {
                        self.establish_cooldown(MAX_RETRY_DELAY);
                        return Err(ReadAttemptError::RateLimited { retry_after_secs });
                    }
                    let delay = retry_after_secs
                        .map_or_else(|| self.retry_delay(attempt), Duration::from_secs);
                    self.establish_cooldown(delay);
                    if attempt == self.max_attempts {
                        return Err(ReadAttemptError::RateLimited {
                            retry_after_secs: Some(reported_delay),
                        });
                    }
                    self.wait_for_retry(delay).await;
                }
                Err(error @ ReadAttemptError::Other(_)) => return Err(error),
            }
            attempt = attempt.saturating_add(1);
        }
    }

    pub(super) async fn wait_for_start(&self) {
        let (ticket, mut changes) = {
            let mut state = self.lock_reservations();
            let ticket = state.next_ticket;
            state.next_ticket = state.next_ticket.wrapping_add(1);
            (ticket, self.changed.subscribe())
        };
        let mut reservation = Reservation {
            policy: self,
            ticket,
            completed: false,
        };

        loop {
            let deadline = {
                let mut state = self.lock_reservations();
                if state.serving_ticket != ticket {
                    None
                } else {
                    let now = self.clock.now();
                    let paced = state
                        .last_start
                        .and_then(|last| last.checked_add(self.request_interval));
                    let deadline = [paced, state.cooldown_until]
                        .into_iter()
                        .flatten()
                        .max()
                        .unwrap_or(now);
                    if now >= deadline {
                        state.last_start = Some(now);
                        state.serving_ticket = state.serving_ticket.wrapping_add(1);
                        Self::skip_cancelled(&mut state);
                        reservation.completed = true;
                        self.signal_change();
                        return;
                    }
                    Some(deadline)
                }
            };

            if let Some(deadline) = deadline {
                self.clock.sleep_until(deadline).await;
            } else if changes.changed().await.is_err() {
                return;
            }
        }
    }

    fn retry_delay(&self, failed_attempt: u32) -> Duration {
        let exponent = failed_attempt.saturating_sub(1).min(31);
        let multiplier = 1_u32 << exponent;
        self.retry_backoff
            .checked_mul(multiplier)
            .unwrap_or(MAX_RETRY_DELAY)
            .min(MAX_RETRY_DELAY)
    }

    async fn wait_for_retry(&self, delay: Duration) {
        if let Some(deadline) = self.clock.now().checked_add(delay.min(MAX_RETRY_DELAY)) {
            self.clock.sleep_until(deadline).await;
        }
    }

    fn establish_cooldown(&self, delay: Duration) {
        let Some(deadline) = self.clock.now().checked_add(delay.min(MAX_RETRY_DELAY)) else {
            return;
        };
        let mut state = self.lock_reservations();
        if state
            .cooldown_until
            .is_none_or(|current| current < deadline)
        {
            state.cooldown_until = Some(deadline);
            self.signal_change();
        }
    }

    fn cancel(&self, ticket: u64) {
        let mut state = self.lock_reservations();
        if ticket >= state.serving_ticket {
            state.cancelled.insert(ticket);
            Self::skip_cancelled(&mut state);
            self.signal_change();
        }
    }

    fn skip_cancelled(state: &mut ReservationState) {
        while state.cancelled.remove(&state.serving_ticket) {
            state.serving_ticket = state.serving_ticket.wrapping_add(1);
        }
    }

    fn lock_reservations(&self) -> MutexGuard<'_, ReservationState> {
        self.reservations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn signal_change(&self) {
        self.changed
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
}

fn parse_http_date(value: &str) -> Option<DateTime<Utc>> {
    const IMF_FIXDATE: &str = "%a, %d %b %Y %H:%M:%S GMT";
    const RFC850_DATE: &str = "%A, %d-%b-%y %H:%M:%S GMT";
    const ASCTIME_DATE: &str = "%a %b %e %H:%M:%S %Y";

    [IMF_FIXDATE, RFC850_DATE, ASCTIME_DATE]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value.trim(), format).ok())
        .map(|date| date.and_utc())
}

struct Reservation<'a> {
    policy: &'a ReadTransportPolicy,
    ticket: u64,
    completed: bool,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.policy.cancel(self.ticket);
        }
    }
}

#[cfg(test)]
pub(super) struct ManualClock {
    state: Mutex<ManualClockState>,
}

#[cfg(test)]
struct ManualClockState {
    now: Instant,
    wall_now: SystemTime,
    sleepers: Vec<(Instant, tokio::sync::oneshot::Sender<()>)>,
}

#[cfg(test)]
impl ManualClock {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(ManualClockState {
                now: Instant::now(),
                wall_now: SystemTime::UNIX_EPOCH
                    .checked_add(Duration::from_millis(1_700_000_000_500))
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                sleepers: Vec::new(),
            }),
        }
    }

    pub(super) fn advance(&self, duration: Duration) {
        let ready = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.now = state
                .now
                .checked_add(duration)
                .unwrap_or_else(|| panic!("manual clock overflow while advancing by {duration:?}"));
            state.wall_now = state.wall_now.checked_add(duration).unwrap_or_else(|| {
                panic!("manual wall clock overflow while advancing by {duration:?}")
            });
            let now = state.now;
            let mut ready = Vec::new();
            let mut pending = Vec::new();
            for (deadline, sender) in state.sleepers.drain(..) {
                if deadline <= now {
                    ready.push(sender);
                } else {
                    pending.push((deadline, sender));
                }
            }
            state.sleepers = pending;
            ready
        };
        for sender in ready {
            match sender.send(()) {
                Ok(()) | Err(()) => {}
            }
        }
    }

    pub(super) fn pending_sleepers(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sleepers
            .len()
    }
}

#[cfg(test)]
impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .now
    }

    fn wall_now(&self) -> SystemTime {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .wall_now
    }

    fn sleep_until(&self, deadline: Instant) -> SleepFuture<'_> {
        Box::pin(async move {
            loop {
                let receiver = {
                    let mut state = self
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if state.now >= deadline {
                        return;
                    }
                    let (sender, receiver) = tokio::sync::oneshot::channel();
                    state.sleepers.push((deadline, sender));
                    receiver
                };
                drop(receiver.await);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn policy(clock: Arc<ManualClock>) -> Arc<ReadTransportPolicy> {
        Arc::new(ReadTransportPolicy::new(&HttpConfig::default(), clock))
    }

    #[test]
    fn parses_all_http_date_forms_as_the_same_utc_instant() {
        for value in [
            "Tue, 14 Nov 2023 22:13:30 GMT",
            "Tuesday, 14-Nov-23 22:13:30 GMT",
            "Tue Nov 14 22:13:30 2023",
        ] {
            assert_eq!(
                parse_http_date(value).map(|date| date.timestamp()),
                Some(1_700_000_010),
                "HTTP date form {value:?}"
            );
        }
    }

    #[test]
    fn rfc850_two_digit_years_use_chronos_pivot_and_validate_weekdays() {
        for (value, expected) in [
            ("Wednesday, 14-Nov-68 22:13:30 GMT", 3_120_156_810),
            ("Thursday, 14-Nov-69 22:13:30 GMT", 3_151_692_810),
            ("Saturday, 14-Nov-70 22:13:30 GMT", 27_468_810),
        ] {
            assert_eq!(
                parse_http_date(value).map(|date| date.timestamp()),
                Some(expected),
                "RFC850 date {value:?}"
            );
        }
        assert!(parse_http_date("Monday, 14-Nov-23 22:13:30 GMT").is_none());
    }

    #[test]
    fn retry_after_parsing_preserves_whitespace_errors_rounding_and_cap_boundary() {
        let policy = policy(Arc::new(ManualClock::new()));

        for (value, expected) in [
            ("  17\t", Some(17)),
            ("soon", None),
            ("Tue, 14 Nov 2023 22:13:19 GMT", Some(0)),
            ("Tue, 14 Nov 2023 22:13:20 GMT", Some(0)),
            ("Tue, 14 Nov 2023 22:13:21 GMT", Some(1)),
            ("Tue, 14 Nov 2023 22:14:20 GMT", Some(60)),
            ("Tue, 14 Nov 2023 22:14:21 GMT", Some(61)),
            ("  Tue, 14 Nov 2023 22:13:22 GMT\t", Some(2)),
        ] {
            assert_eq!(
                policy.retry_after_delay_secs(value),
                expected,
                "Retry-After value {value:?}"
            );
        }
    }

    async fn wait_for_count(count: &AtomicUsize, expected: usize) {
        for _ in 0..100 {
            if count.load(Ordering::SeqCst) == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!(
            "expected {expected} attempts, observed {}",
            count.load(Ordering::SeqCst)
        );
    }

    async fn wait_for_sleepers(clock: &ManualClock, expected: usize) {
        for _ in 0..100 {
            if clock.pending_sleepers() >= expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!(
            "expected at least {expected} sleepers, observed {}",
            clock.pending_sleepers()
        );
    }

    #[tokio::test]
    async fn connection_and_timeout_failures_retry_with_exponential_backoff() {
        let clock = Arc::new(ManualClock::new());
        let policy = policy(clock.clone());
        let attempts = Arc::new(AtomicUsize::new(0));
        let task_attempts = attempts.clone();
        let task = tokio::spawn(async move {
            policy
                .execute(|| {
                    let attempt = task_attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    async move {
                        match attempt {
                            1 => Err(ReadAttemptError::Connection("connect")),
                            2 => Err(ReadAttemptError::Timeout("timeout")),
                            _ => Ok("ok"),
                        }
                    }
                })
                .await
        });

        wait_for_count(&attempts, 1).await;
        wait_for_sleepers(&clock, 1).await;
        clock.advance(Duration::from_millis(249));
        tokio::task::yield_now().await;
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        clock.advance(Duration::from_millis(1));
        wait_for_count(&attempts, 2).await;

        wait_for_sleepers(&clock, 1).await;
        clock.advance(Duration::from_millis(499));
        tokio::task::yield_now().await;
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        clock.advance(Duration::from_millis(1));
        wait_for_count(&attempts, 3).await;

        assert_eq!(task.await.unwrap().unwrap(), "ok");
    }

    #[tokio::test]
    async fn retry_after_sets_shared_cooldown_across_clones() {
        let clock = Arc::new(ManualClock::new());
        let policy = policy(clock.clone());
        let limited_attempts = Arc::new(AtomicUsize::new(0));
        let task_attempts = limited_attempts.clone();
        let limited_policy = policy.clone();
        let limited = tokio::spawn(async move {
            limited_policy
                .execute(|| {
                    let attempt = task_attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    async move {
                        if attempt == 1 {
                            Err(ReadAttemptError::<()>::RateLimited {
                                retry_after_secs: Some(2),
                            })
                        } else {
                            Ok("retried")
                        }
                    }
                })
                .await
        });
        wait_for_count(&limited_attempts, 1).await;
        wait_for_sleepers(&clock, 1).await;

        let clone_attempts = Arc::new(AtomicUsize::new(0));
        let task_attempts = clone_attempts.clone();
        let clone_policy = policy.clone();
        let concurrent = tokio::spawn(async move {
            clone_policy
                .execute(|| {
                    task_attempts.fetch_add(1, Ordering::SeqCst);
                    async { Ok::<_, ReadAttemptError<()>>("concurrent") }
                })
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(clone_attempts.load(Ordering::SeqCst), 0);

        clock.advance(Duration::from_millis(1_999));
        tokio::task::yield_now().await;
        assert_eq!(clone_attempts.load(Ordering::SeqCst), 0);
        clock.advance(Duration::from_millis(1));
        wait_for_count(&clone_attempts, 1).await;
        assert_eq!(concurrent.await.unwrap().unwrap(), "concurrent");

        wait_for_sleepers(&clock, 1).await;
        clock.advance(Duration::from_millis(250));
        wait_for_count(&limited_attempts, 2).await;
        assert_eq!(limited.await.unwrap().unwrap(), "retried");
    }

    #[tokio::test]
    async fn retry_after_above_cap_is_terminal_and_caps_shared_cooldown() {
        let clock = Arc::new(ManualClock::new());
        let policy = policy(clock.clone());
        let attempts = AtomicUsize::new(0);

        let error = policy
            .execute(|| {
                attempts.fetch_add(1, Ordering::SeqCst);
                async {
                    Err::<(), _>(ReadAttemptError::<()>::RateLimited {
                        retry_after_secs: Some(61),
                    })
                }
            })
            .await
            .err()
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(matches!(
            error,
            ReadAttemptError::RateLimited {
                retry_after_secs: Some(61)
            }
        ));

        let concurrent_attempts = Arc::new(AtomicUsize::new(0));
        let task_attempts = concurrent_attempts.clone();
        let task_policy = policy.clone();
        let concurrent = tokio::spawn(async move {
            task_policy
                .execute(|| {
                    task_attempts.fetch_add(1, Ordering::SeqCst);
                    async { Ok::<_, ReadAttemptError<()>>(()) }
                })
                .await
        });
        tokio::task::yield_now().await;
        clock.advance(Duration::from_millis(59_999));
        tokio::task::yield_now().await;
        assert_eq!(concurrent_attempts.load(Ordering::SeqCst), 0);
        clock.advance(Duration::from_millis(1));
        wait_for_count(&concurrent_attempts, 1).await;
        concurrent.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn non_retryable_failures_execute_once() {
        let clock = Arc::new(ManualClock::new());
        let policy = policy(clock);
        let attempts = AtomicUsize::new(0);

        let result = policy
            .execute(|| {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err::<(), _>(ReadAttemptError::Other("api")) }
            })
            .await;

        assert!(matches!(result, Err(ReadAttemptError::Other("api"))));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelled_reservation_does_not_leave_a_pacing_gap() {
        let clock = Arc::new(ManualClock::new());
        let policy = policy(clock.clone());
        policy.wait_for_start().await;

        let cancelled_policy = policy.clone();
        let cancelled = tokio::spawn(async move { cancelled_policy.wait_for_start().await });
        wait_for_sleepers(&clock, 1).await;

        let survivor_policy = policy.clone();
        let survivor = tokio::spawn(async move { survivor_policy.wait_for_start().await });
        tokio::task::yield_now().await;
        cancelled.abort();
        drop(cancelled.await);

        clock.advance(Duration::from_millis(250));
        for _ in 0..100 {
            if survivor.is_finished() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            survivor.is_finished(),
            "surviving reservation did not start"
        );
        survivor.await.unwrap();
    }
}
