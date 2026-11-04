//! Restore the persisted model if any activation phase fails before commit.
use crate::{
    stt::{RecoverySelection, SttEngine, WorkerPaths},
    types::Settings,
};
use std::sync::Arc;
pub(crate) struct SelectionRollback {
    engine: Arc<SttEngine>,
    previous: Settings,
    paths: WorkerPaths,
    committed: bool,
}
impl SelectionRollback {
    pub(crate) fn new(engine: Arc<SttEngine>, previous: Settings, paths: WorkerPaths) -> Self {
        Self {
            engine,
            previous,
            paths,
            committed: false,
        }
    }
    pub(crate) fn commit(&mut self) {
        self.committed = true;
    }
}
impl Drop for SelectionRollback {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if self.previous.recognition_model_id().is_none() {
            self.engine
                .reject_prepared_selection("Модель не выбрана. Выберите модель заново.");
            return;
        }
        if self
            .engine
            .ensure_configured(
                &self.previous,
                &self.paths,
                &fono_core::OperationCancellation::default(),
            )
            .is_err()
        {
            self.engine.reject_prepared_selection(
                "Не удалось восстановить модель после ошибки сохранения. Выберите модель заново.",
            );
        } else if let Some(model) = self
            .previous
            .selected_stt_model()
            .filter(|_| self.previous.stt_source == crate::types::SttSource::Local)
        {
            if let Some(prepared) = self.engine.prepared_recovery_selection(&RecoverySelection {
                model_path: model.local_path.into(),
                acceleration: self.previous.acceleration,
                worker_paths: self.paths.clone(),
            }) {
                self.engine.acknowledge_recovery_selection(prepared);
            }
        }
    }
}
