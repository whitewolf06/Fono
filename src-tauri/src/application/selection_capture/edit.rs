//! Each edit owns its cancellation and actual work independently of IPC waiters.
use super::*;
use crate::llm::{selection::validate_instruction, LlmClient};
use std::sync::{atomic::AtomicBool, Arc};

mod voice;

#[derive(Clone, Serialize)]
pub(crate) struct EditSnapshot {
    phase: &'static str,
    operation_id: Option<u64>,
    instruction: String,
    result: Option<String>,
    result_id: Option<u64>,
    message: Option<String>,
}

#[derive(Clone, Default)]
struct Control {
    cancel: OperationCancellation,
    stop: Arc<AtomicBool>,
}
struct Job {
    id: u64,
    phase: &'static str,
    control: Control,
}
#[derive(Default)]
pub(super) struct Editor {
    instruction: String,
    result: Option<String>,
    result_id: Option<u64>,
    result_current: bool,
    message: Option<String>,
    job: Option<Job>,
}
impl Editor {
    pub(super) fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            phase: self.job.as_ref().map_or_else(
                || {
                    if self.result.is_some() {
                        "ready"
                    } else {
                        "idle"
                    }
                },
                |j| {
                    if j.control.cancel.is_cancelled() {
                        "cancelling"
                    } else {
                        j.phase
                    }
                },
            ),
            operation_id: self.job.as_ref().map(|j| j.id),
            instruction: self.instruction.clone(),
            result: self.result.clone(),
            result_id: self.result_id,
            message: self.message.clone(),
        }
    }
    fn begin(&mut self, id: u64, phase: &'static str) -> AppResult<Control> {
        if self.job.is_some() {
            return Err(AppError::Busy(
                "Дождитесь завершения текущего действия с выделением.".into(),
            ));
        }
        let control = Control::default();
        self.message = None;
        self.result_current = false;
        self.job = Some(Job {
            id,
            phase,
            control: control.clone(),
        });
        Ok(control)
    }
    pub(super) fn cancel(&self) {
        if let Some(j) = &self.job {
            j.control.cancel.cancel();
        }
    }
    fn complete(&mut self, id: u64, voice: bool, value: AppResult<String>) -> bool {
        let Some(job) = self.job.as_ref().filter(|j| j.id == id) else {
            return false;
        };
        if !job.control.cancel.is_cancelled() {
            match value {
                Ok(value) if voice => {
                    self.instruction = value;
                    self.result = None;
                    self.result_id = None;
                }
                Ok(value) => {
                    self.result = Some(value);
                    self.result_id = Some(id);
                    self.result_current = true;
                }
                Err(error) => {
                    self.message = Some(error.user_error().params.detail.unwrap_or_else(|| {
                        "Не удалось преобразовать выделение. Оригинал сохранён; повторите действие."
                            .into()
                    }))
                }
            }
        }
        self.job = None;
        true
    }
    pub(super) fn can_replace(&self) -> bool {
        self.job.is_none() && self.result_current && self.result_id.is_some()
    }
    pub(super) fn output(&self, result_id: u64, fresh: bool) -> AppResult<String> {
        if self.job.is_some()
            || self.result_id != Some(result_id)
            || (fresh && !self.result_current)
        {
            return Err(AppError::Cancelled(
                "Результат уже изменился. Повторите преобразование выделения.".into(),
            ));
        }
        self.result.clone().ok_or_else(stale)
    }
}

struct Work {
    selection: u64,
    id: u64,
    material: String,
    control: Control,
    _lease: ActivityLease<'static>,
}
fn begin(app: &AppHandle, selection: u64, phase: &'static str) -> AppResult<Work> {
    let lease = activity::lease()?;
    let runtime = app.state::<Runtime>();
    let mut slot = runtime.slot.lock();
    let a = slot
        .active
        .as_mut()
        .filter(|a| a.id == selection)
        .ok_or_else(stale)?;
    a.delivery.invalidate_prepared()?;
    let material = match a.result.as_ref() {
        Some(Ok(value)) if value.belongs_to(selection) => value.text.clone(),
        _ => return Err(stale()),
    };
    let id = runtime.next.fetch_add(1, Ordering::Relaxed) + 1;
    let control = a.edit.begin(id, phase)?;
    Ok(Work {
        selection,
        id,
        material,
        control,
        _lease: lease,
    })
}
fn stale() -> AppError {
    AppError::Cancelled("Это выделение уже закрыто. Получите его заново.".into())
}
fn with_editor<T>(
    app: &AppHandle,
    selection: u64,
    action: impl FnOnce(&mut Editor) -> T,
) -> Option<T> {
    let runtime = app.state::<Runtime>();
    let mut slot = runtime.slot.lock();
    let a = slot.active.as_mut().filter(|a| a.id == selection)?;
    Some(action(&mut a.edit))
}
fn phase(app: &AppHandle, work: &Work, phase: &'static str) {
    with_editor(app, work.selection, |e| {
        if let Some(job) = e.job.as_mut().filter(|j| j.id == work.id) {
            job.phase = phase;
        }
    });
    notify(app);
}
fn complete(app: &AppHandle, work: &Work, voice: bool, result: AppResult<String>) {
    if with_editor(app, work.selection, |e| e.complete(work.id, voice, result)) == Some(true) {
        if !voice {
            delivery::auto_submit(app, work.selection, work.id);
        }
        notify(app);
    }
}

pub(crate) fn start_voice(app: AppHandle, selection: u64) -> AppResult<()> {
    let work = begin(&app, selection, "starting")?;
    notify(&app);
    tauri::async_runtime::spawn(async move {
        let result = voice::run(&app, &work).await;
        complete(&app, &work, true, result);
    });
    Ok(())
}
pub(crate) fn stop_voice(app: &AppHandle, selection: u64, operation: u64) {
    with_editor(app, selection, |e| {
        if let Some(j) = e
            .job
            .as_ref()
            .filter(|j| j.id == operation && matches!(j.phase, "starting" | "recording"))
        {
            j.control.stop.store(true, Ordering::Release);
        }
    });
}
pub(crate) fn cancel(app: &AppHandle, selection: u64, operation: u64) {
    with_editor(app, selection, |e| {
        if let Some(j) = e.job.as_ref().filter(|j| j.id == operation) {
            j.control.cancel.cancel();
        }
    });
    notify(app);
}

pub(crate) fn transform(
    app: AppHandle,
    selection: u64,
    instruction: String,
    transfer_confirmed: bool,
) -> AppResult<()> {
    if !transfer_confirmed {
        return Err(AppError::Config(
            "Подтвердите отправку выделения и инструкции в выбранное LLM-подключение.".into(),
        ));
    }
    validate_instruction(&instruction)?;
    let work = begin(&app, selection, "processing")?;
    with_editor(&app, selection, |e| {
        e.instruction = instruction.clone();
    });
    notify(&app);
    tauri::async_runtime::spawn(async move {
        let result = process(&app, &work, &instruction).await;
        complete(&app, &work, false, result);
    });
    Ok(())
}

async fn process(app: &AppHandle, work: &Work, instruction: &str) -> AppResult<String> {
    if work.control.cancel.is_cancelled() {
        return Err(stale());
    }
    let settings = app.state::<AppState>().settings();
    let profile = settings.correction_profile().cloned().ok_or_else(|| {
        AppError::Config("Выберите подключение и модель для обработки текста".into())
    })?;
    let model = settings.text_correction_llm.model.clone();
    let lease = work._lease.clone();
    let client = tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        LlmClient::from_profile(&profile, model.as_deref())
    })
    .await
    .map_err(|_| AppError::Internal("Не удалось подготовить преобразование выделения.".into()))?;
    if work.control.cancel.is_cancelled() {
        return Err(stale());
    }
    tokio::select! {
        result = client.process_selection(&work.material, instruction) => result,
        _ = crate::application::dictation::wait_for_cancellation(work.control.cancel.clone()) => Err(stale()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelling_does_not_release_the_job_before_its_real_cleanup() {
        let mut e = Editor {
            instruction: "previous".into(),
            result: Some("previous result".into()),
            ..Default::default()
        };
        let control = e.begin(1, "processing").unwrap();
        e.cancel();
        assert!(control.cancel.is_cancelled());
        assert_eq!(e.snapshot().phase, "cancelling");
        assert!(e.begin(2, "starting").is_err());
        assert!(e.complete(1, false, Ok("late result".into())));
        assert_eq!(e.instruction, "previous");
        assert_eq!(e.result.as_deref(), Some("previous result"));
        assert!(e.begin(2, "starting").is_ok());
        assert!(!e.complete(1, false, Ok("stale".into())));
        assert_eq!(e.snapshot().operation_id, Some(2));
    }
    #[test]
    fn instruction_and_material_result_have_distinct_lifecycles() {
        let mut e = Editor::default();
        e.begin(1, "recording").unwrap();
        e.complete(1, true, Ok("Сделай формальнее".into()));
        assert!(e.result.is_none());
        assert_eq!(e.instruction, "Сделай формальнее");
        e.begin(2, "processing").unwrap();
        e.complete(2, false, Ok("Уважаемые коллеги".into()));
        assert_eq!(e.instruction, "Сделай формальнее");
        assert_eq!(e.snapshot().phase, "ready");
        e.begin(3, "processing").unwrap();
        e.complete(3, false, Err(AppError::Llm("PRIVATE URL AND TEXT".into())));
        assert!(!e.snapshot().message.unwrap().contains("PRIVATE"));
        assert_eq!(e.result.as_deref(), Some("Уважаемые коллеги"));
    }
    #[test]
    fn only_the_latest_successful_result_can_be_armed_after_failure_or_cancel() {
        let mut e = Editor::default();
        e.begin(1, "processing").unwrap();
        e.complete(1, false, Ok("first result".into()));
        assert_eq!(e.output(1, true).unwrap(), "first result");
        assert!(e.output(2, true).is_err());
        e.begin(2, "processing").unwrap();
        assert!(e.output(1, false).is_err());
        e.complete(2, false, Err(AppError::Llm("error".into())));
        assert!(e.output(1, true).is_err());
        assert_eq!(e.output(1, false).unwrap(), "first result");
        e.begin(3, "processing").unwrap();
        e.cancel();
        e.complete(3, false, Ok("late cancelled".into()));
        assert!(e.output(1, true).is_err());
        e.begin(4, "processing").unwrap();
        e.complete(4, false, Ok("new result".into()));
        assert!(e.output(1, false).is_err());
        assert_eq!(e.output(4, true).unwrap(), "new result");
    }
}
