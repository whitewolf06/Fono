//! Bounded capture buffers and operation-fenced physical audio ownership.
use super::*;

impl Pipeline {
    pub fn recording_window(&self, from: u64, maximum: usize) -> (u64, Vec<i16>) {
        self.writer
            .lock()
            .as_ref()
            .map(|w| w.lock().window(from, maximum))
            .unwrap_or((from, Vec::new()))
    }
    pub fn recording_end(&self) -> u64 {
        self.writer.lock().as_ref().map_or(0, |w| w.lock().end())
    }
    pub fn recording_start(&self) -> u64 {
        self.writer.lock().as_ref().map_or(0, |w| w.lock().start())
    }
    pub fn discard_audio_before(&self, offset: u64) {
        if let Some(writer) = self.writer.lock().as_ref() {
            writer.lock().discard_before(offset);
        }
    }

    pub fn is_recording(&self) -> bool {
        *self.recording.lock()
    }

    pub fn recording_limit_reached(&self) -> bool {
        self.recording_limit_reached.load(Ordering::SeqCst)
    }

    /// Нормированный RMS уровень звука последних ~100 мс записи (0.0..1.0).
    /// Уровень для визуального индикатора; решения о речи принимает neural VAD.
    /// Возвращает 0.0, если запись не идёт или буфер пока пуст.
    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.audio_level_bits.load(Ordering::Relaxed))
    }

    pub fn start_recording_from(
        &self,
        device_id: Option<&str>,
        source: OperationSource,
    ) -> AppResult<u64> {
        self.start_recording_with_pre_roll_from(device_id, &[], source)
    }

    /// Запускает запись и добавляет короткий фрагмент до старта захвата.
    /// Нужен wake word: detector распознаёт фразу с задержкой, а pre-roll
    /// сохраняет слова, которые пользователь произнёс сразу после неё.
    pub fn start_recording_with_pre_roll_from(
        &self,
        device_id: Option<&str>,
        pre_roll: &[i16],
        source: OperationSource,
    ) -> AppResult<u64> {
        self.start_capture(device_id, pre_roll, source, None, false)
    }

    pub fn start_capture(
        &self,
        device_id: Option<&str>,
        pre_roll: &[i16],
        source: OperationSource,
        cursor: Option<(u64, u64)>,
        live: bool,
    ) -> AppResult<u64> {
        let _update_admission = crate::application::updates::activity::begin()?;
        let mut recording = self.recording.lock();
        if *recording {
            tracing::warn!("start_recording called while already recording");
            return Err(AppError::Busy("audio recording is already active".into()));
        }
        let operation = self.operations.start(source)?;
        self.operations
            .acquire_resource(operation.id, OperationResource::Audio)?;
        if pre_roll.len() > MAX_RECORDING_SAMPLES {
            let _ = self.operations.finish(operation.id, TerminalReason::Failed);
            return Err(AppError::Audio(format!(
                "pre-roll exceeds the maximum recording length of {MAX_RECORDING_SECONDS} seconds"
            )));
        }

        self.recording_limit_reached.store(false, Ordering::SeqCst);
        let maximum = if live {
            RECORDING_SAMPLE_RATE * 120
        } else {
            MAX_RECORDING_SAMPLES
        };
        let writer = Arc::new(Mutex::new(RecordingBuffer::new(
            pre_roll,
            INITIAL_RECORDING_CAPACITY,
        )));
        self.scheduler.reserve(operation.id);
        if let Err(error) = self.audio_owner.start_after(
            device_id,
            Arc::clone(&writer),
            Arc::clone(&self.recording_limit_reached),
            Arc::clone(&self.audio_level_bits),
            maximum,
            cursor,
        ) {
            self.scheduler.release_reservation(operation.id);
            let _ = self.operations.finish(operation.id, TerminalReason::Failed);
            return Err(error);
        }

        *self.writer.lock() = Some(writer);
        *self.stopped_capture_drops.lock() = None;
        self.capture_operation
            .store(operation.id, Ordering::Release);
        *recording = true;
        tracing::info!(
            operation = operation.id,
            source = ?source,
            "recording started, bounded writer attached (pre_roll={:.2}s, max={}s)",
            pre_roll.len() as f32 / RECORDING_SAMPLE_RATE as f32,
            MAX_RECORDING_SECONDS
        );
        Ok(operation.id)
    }

    /// Collect only the writer owned by this operation, under the capture lock.
    pub fn stop_recording_for(&self, operation: u64) -> AppResult<Option<Vec<i16>>> {
        let mut recording = self.recording.lock();
        if operation == 0 || self.capture_operation.load(Ordering::Acquire) != operation {
            return Ok(None);
        }
        self.stop_capture_locked(&mut recording)?;
        let writer = self.writer.lock().take();
        self.capture_operation.store(0, Ordering::Release);
        Ok(writer.map(|writer| writer.lock().take()))
    }

    pub fn stop_recording(&self) -> AppResult<Vec<i16>> {
        let operation = self.capture_operation.load(Ordering::Acquire);
        Ok(self.stop_recording_for(operation)?.unwrap_or_default())
    }

    /// Final capture loss belongs to exactly one still-current operation. A late
    /// collector must never read the replacement recording's counter or old loss.
    pub(crate) fn capture_dropped_packets_for(&self, operation: u64) -> Option<u64> {
        let _capture = self.recording.lock();
        if operation == 0 || self.operation_id() != operation {
            return None;
        }
        self.stopped_capture_drops
            .lock()
            .as_ref()
            .filter(|(owner, _)| *owner == operation)
            .map(|(_, count)| *count)
    }

    /// Physical capture stops immediately; its queued packets drain before the
    /// lock is released. The live decoder retains its writer for final windows.
    pub fn stop_capture_for(&self, operation: u64) -> AppResult<bool> {
        let mut recording = self.recording.lock();
        if operation == 0 || self.capture_operation.load(Ordering::Acquire) != operation {
            return Ok(false);
        }
        self.stop_capture_locked(&mut recording)?;
        Ok(true)
    }

    pub fn stop_capture(&self) -> AppResult<()> {
        self.stop_capture_for(self.capture_operation.load(Ordering::Acquire))
            .map(|_| ())
    }

    fn stop_capture_locked(&self, recording: &mut bool) -> AppResult<()> {
        if !*recording {
            return Ok(());
        }
        let operation = self.capture_operation.load(Ordering::Acquire);
        #[cfg(feature = "speech-vad")]
        self.capture_segments.stop(operation);
        self.scheduler.promote_stop(operation);
        let stopped = self.audio_owner.stop();
        *self.stopped_capture_drops.lock() = self
            .audio_owner
            .dropped_capture_packets()
            .map(|count| (self.capture_operation.load(Ordering::Acquire), count));
        *recording = false;
        self.audio_level_bits
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.operations.release_resource(
            self.capture_operation.load(Ordering::Acquire),
            OperationResource::Audio,
        );
        stopped?;
        if self.recording_limit_reached() {
            tracing::warn!(
                "recording buffer reached its limit; input stopped with an explicit warning"
            );
        }
        Ok(())
    }
}
