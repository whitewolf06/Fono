//! Request ownership survives a dropped IPC handle; cancellation never reuses IDs.
use crate::error::{AppError, AppResult};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Weak,
    },
};
use tokio::sync::Notify;

const RETIRED_LIMIT: usize = 128;
static SOURCE_CHANGES: Lazy<SourceChanges> = Lazy::new(SourceChanges::default);
static PERIODS: Lazy<Mutex<PeriodRegistry>> = Lazy::new(|| Mutex::new(PeriodRegistry::default()));
static SESSIONS: Lazy<Mutex<HashMap<String, Weak<RequestToken>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Default)]
pub(crate) struct RequestToken {
    cancelled: AtomicBool,
    notify: Notify,
}
impl RequestToken {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    pub(crate) async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            self.notify.notified().await;
        }
    }
}
struct ActivePeriod {
    days: u32,
    token: Arc<RequestToken>,
}

struct SourceChanges {
    epoch: AtomicU64,
    notify: Notify,
}
impl Default for SourceChanges {
    fn default() -> Self {
        Self {
            epoch: AtomicU64::new(1),
            notify: Notify::new(),
        }
    }
}
impl SourceChanges {
    fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }
    fn changed(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
    async fn since(&self, epoch: u64) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.epoch() != epoch {
                return;
            }
            notified.await;
        }
    }
}
#[derive(Default)]
struct PeriodRegistry {
    active: HashMap<String, ActivePeriod>,
    retired: VecDeque<String>,
}
impl PeriodRegistry {
    fn retire(&mut self, id: String) {
        if !self.retired.contains(&id) {
            self.retired.push_back(id);
        }
        while self.retired.len() > RETIRED_LIMIT {
            self.retired.pop_front();
        }
    }
    fn begin(&mut self, id: &str, days: u32) -> AppResult<Arc<RequestToken>> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(AppError::Config(
                "Некорректный идентификатор запроса".into(),
            ));
        }
        if self.retired.iter().any(|old| old == id) {
            return Err(AppError::Cancelled(
                "Этот запрос уже завершён или отменён".into(),
            ));
        }
        if self.active.len() >= 8
            || self.active.contains_key(id)
            || self
                .active
                .values()
                .any(|entry| entry.days == days && !entry.token.is_cancelled())
        {
            return Err(AppError::Busy(
                "Отчёт за этот период уже анализируется".into(),
            ));
        }
        let token = Arc::new(RequestToken::default());
        self.active.insert(
            id.to_owned(),
            ActivePeriod {
                days,
                token: Arc::clone(&token),
            },
        );
        Ok(token)
    }
    fn cancel(&mut self, id: &str) {
        if let Some(entry) = self.active.get(id) {
            entry.token.cancel();
        } else if !id.is_empty() && id.len() <= 128 {
            self.retire(id.to_owned());
        }
    }
}
pub(crate) struct PeriodRequest {
    id: String,
    pub(crate) token: Arc<RequestToken>,
}
impl PeriodRequest {
    pub(crate) fn begin(id: String, days: u32) -> AppResult<Self> {
        let token = PERIODS.lock().begin(&id, days)?;
        Ok(Self { id, token })
    }
    pub(crate) fn commit<T>(&self, action: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        // Serialize cancellation with the final local commit. History callbacks
        // only signal SOURCE_EPOCH and never acquire this registry.
        let registry = PERIODS.lock();
        let valid = registry
            .active
            .get(&self.id)
            .is_some_and(|entry| Arc::ptr_eq(&entry.token, &self.token));
        if !valid || self.token.is_cancelled() {
            return Err(AppError::Cancelled("Анализ отчёта отменён".into()));
        }
        action()
    }
}
impl Drop for PeriodRequest {
    fn drop(&mut self) {
        let mut registry = PERIODS.lock();
        if registry
            .active
            .get(&self.id)
            .is_some_and(|entry| Arc::ptr_eq(&entry.token, &self.token))
        {
            registry.active.remove(&self.id);
            registry.retire(self.id.clone());
        }
    }
}
pub(crate) fn cancel_period(id: &str) {
    PERIODS.lock().cancel(id);
}

pub(crate) fn source_epoch() -> u64 {
    SOURCE_CHANGES.epoch()
}
pub(crate) fn sources_changed() {
    SOURCE_CHANGES.changed();
}
pub(crate) async fn changed_since(epoch: u64) {
    SOURCE_CHANGES.since(epoch).await;
}

pub(crate) struct SessionRequest {
    id: String,
    pub(crate) token: Arc<RequestToken>,
}
impl SessionRequest {
    pub(crate) fn begin(id: &str) -> AppResult<Self> {
        let mut sessions = SESSIONS.lock();
        if sessions
            .get(id)
            .and_then(Weak::upgrade)
            .is_some_and(|token| !token.is_cancelled())
        {
            return Err(AppError::Busy("Диктовка уже анализируется".into()));
        }
        let token = Arc::new(RequestToken::default());
        sessions.insert(id.to_owned(), Arc::downgrade(&token));
        Ok(Self {
            id: id.to_owned(),
            token,
        })
    }
}
impl Drop for SessionRequest {
    fn drop(&mut self) {
        let mut sessions = SESSIONS.lock();
        if sessions
            .get(&self.id)
            .and_then(Weak::upgrade)
            .is_some_and(|token| Arc::ptr_eq(&token, &self.token))
        {
            sessions.remove(&self.id);
        }
    }
}
pub(crate) fn invalidate_session(id: &str) {
    if let Some(token) = SESSIONS.lock().get(id).and_then(Weak::upgrade) {
        token.cancel();
    }
}
pub(crate) fn invalidate_all_sessions() {
    for token in SESSIONS.lock().values().filter_map(Weak::upgrade) {
        token.cancel();
    }
}
pub(crate) fn settings_changed() {
    sources_changed();
    invalidate_all_sessions();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_before_start_and_reused_ids_cannot_start_late_work() {
        let mut registry = PeriodRegistry::default();
        registry.cancel("cancelled-before-start");
        assert!(matches!(
            registry.begin("cancelled-before-start", 7),
            Err(AppError::Cancelled(_))
        ));
        let token = registry.begin("active", 7).unwrap();
        assert!(registry.begin("other", 7).is_err());
        registry.cancel("active");
        assert!(token.is_cancelled());
        assert!(registry.begin("replacement", 7).is_ok());
    }
    #[test]
    fn cancellation_does_not_touch_another_period_and_tombstones_are_bounded() {
        let mut registry = PeriodRegistry::default();
        let first = registry.begin("first", 7).unwrap();
        let second = registry.begin("second", 30).unwrap();
        registry.cancel("first");
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        for index in 0..(RETIRED_LIMIT * 2) {
            registry.cancel(&format!("retired-{index}"));
        }
        assert_eq!(registry.retired.len(), RETIRED_LIMIT);
    }
    #[test]
    fn cancelled_request_cannot_commit_even_if_a_result_is_already_ready() {
        let id = uuid::Uuid::new_v4().to_string();
        let request = PeriodRequest::begin(id.clone(), 90).unwrap();
        cancel_period(&id);
        assert!(request.commit(|| Ok(())).is_err());
    }
    #[test]
    fn cancellation_notification_is_not_lost_before_await() {
        let token = RequestToken::default();
        token.cancel();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(100), token.cancelled())
                .await
                .unwrap();
        });
    }

    #[test]
    fn source_change_before_subscription_is_detected_without_lost_notification() {
        let changes = SourceChanges::default();
        let epoch = changes.epoch();
        changes.changed();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(100), changes.since(epoch))
                .await
                .unwrap();
        });
    }

    #[test]
    fn detached_async_work_retains_admission_until_actual_cancellation_finishes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let token = Arc::new(RequestToken::default());
            let worker_token = Arc::clone(&token);
            let lease = crate::application::updates::activity::lease().unwrap();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
            let handle = tokio::spawn(async move {
                started_tx.send(()).unwrap();
                worker_token.cancelled().await;
                drop(lease);
                finished_tx.send(()).unwrap();
            });
            started_rx.await.unwrap();
            drop(handle); // Same ownership rule as a dropped Tauri IPC future.
            assert!(crate::application::updates::activity::is_busy());
            token.cancel();
            tokio::time::timeout(std::time::Duration::from_secs(1), finished_rx)
                .await
                .unwrap()
                .unwrap();
        });
    }
}
