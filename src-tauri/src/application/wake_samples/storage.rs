use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::STANDARD, Engine};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};
use uuid::Uuid;

static STORE: Mutex<()> = Mutex::new(());
const RATE: usize = 16_000;
const MAX_RECORDINGS: usize = 200;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Language {
    Ru,
    En,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhraseId {
    // Missing identity belongs to the original dataset, never to the current phrase.
    #[default]
    HeyFono,
    PrivetLira,
}
impl PhraseId {
    fn text(&self, language: &Language) -> AppResult<&'static str> {
        match (self, language) {
            (Self::HeyFono, Language::Ru) => Ok("Эй, Фоно"),
            (Self::HeyFono, Language::En) => Ok("Hey Fono"),
            (Self::PrivetLira, Language::Ru) => Ok("Привет, Лира"),
            _ => Err(AppError::Config(
                "Для «Привет, Лира» выберите русский язык записи.".into(),
            )),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SampleKind {
    Phrase,
    OtherSpeech,
    Background,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Condition {
    Quiet,
    Noisy,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SampleRequest {
    #[serde(default)]
    pub phrase_id: PhraseId,
    pub language: Language,
    pub kind: SampleKind,
    pub condition: Condition,
    pub consent: bool,
}
impl SampleRequest {
    pub fn validate(&self) -> AppResult<()> {
        if !self.consent {
            return Err(AppError::Config(
                "Подтвердите локальное сохранение образцов голоса.".into(),
            ));
        }
        self.phrase_id.text(&self.language)?;
        Ok(())
    }
    pub fn duration_ms(&self) -> u64 {
        match self.kind {
            SampleKind::Phrase => 5_000,
            SampleKind::OtherSpeech | SampleKind::Background => 20_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sample {
    pub id: String,
    #[serde(default)]
    pub phrase_id: PhraseId,
    pub language: Language,
    pub kind: SampleKind,
    pub condition: Condition,
    pub duration_ms: u64,
    pub sha256: String,
    pub recorded_at_ms: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Recording {
    schema: String,
    sample: Sample,
    #[serde(default)]
    phrase: Option<String>,
    speaker: String,
    session: String,
    source_group: String,
    split: String,
    permission: String,
    purpose: String,
    annotation: Option<serde_json::Value>,
}
impl Recording {
    fn validate(&self, id: &str) -> AppResult<()> {
        let phrase = self.sample.phrase_id.text(&self.sample.language)?;
        let identity_matches = match self.schema.as_str() {
            "fono-wake-recording@1" => self.sample.phrase_id == PhraseId::HeyFono,
            "fono-wake-recording@2" => self.phrase.as_deref() == Some(phrase),
            _ => false,
        };
        if self.sample.id != id || !identity_matches {
            return Err(AppError::Config("Образец повреждён или недоступен.".into()));
        }
        Ok(())
    }
}

fn root() -> AppResult<std::path::PathBuf> {
    Ok(crate::state::app_data_dir()?.join("wake-recordings"))
}
fn regular_dir(path: &Path) -> AppResult<()> {
    fs::create_dir_all(path)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() || reparse(&meta) {
        return Err(AppError::Config("Каталог образцов недоступен.".into()));
    }
    Ok(())
}
fn reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}
fn regular_file(path: &Path, maximum: u64) -> AppResult<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || reparse(&meta) || meta.len() > maximum {
        return Err(AppError::Config("Образец повреждён или недоступен.".into()));
    }
    Ok(fs::read(path)?)
}
fn identifier(id: &str) -> AppResult<()> {
    if Uuid::parse_str(id).is_err() || id.len() != 36 || id != id.to_ascii_lowercase() {
        return Err(AppError::Config("Образец не найден.".into()));
    }
    Ok(())
}
fn entry_dir(root: &Path, id: &str) -> AppResult<std::path::PathBuf> {
    identifier(id)?;
    let path = root.join(id);
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() || reparse(&meta) {
        return Err(AppError::Config("Образец повреждён или недоступен.".into()));
    }
    Ok(path)
}
fn load(root: &Path) -> AppResult<Vec<Sample>> {
    let mut items = Vec::new();
    if !root.exists() {
        return Ok(items);
    }
    regular_dir(root)?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if identifier(&id).is_err() {
            continue;
        }
        let dir = entry_dir(root, &id)?;
        let recording: Recording =
            serde_json::from_slice(&regular_file(&dir.join("recording.json"), 16_384)?)?;
        recording.validate(&id)?;
        items.push(recording.sample);
    }
    items.sort_by(|a, b| {
        b.recorded_at_ms
            .cmp(&a.recorded_at_ms)
            .then(a.id.cmp(&b.id))
    });
    Ok(items)
}
pub(crate) fn list() -> AppResult<Vec<Sample>> {
    let _guard = STORE.lock();
    load(&root()?)
}
fn wav(samples: &[i16]) -> AppResult<Vec<u8>> {
    if samples.len() < RATE / 4 || samples.len() > RATE * 22 {
        return Err(AppError::Audio(
            "Запись слишком короткая или превышает лимит. Повторите образец.".into(),
        ));
    }
    let size = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(size as usize + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(size + 36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    out.extend_from_slice(&(RATE as u32).to_le_bytes());
    out.extend_from_slice(&((RATE * 2) as u32).to_le_bytes());
    out.extend_from_slice(b"\x02\0\x10\0data");
    out.extend_from_slice(&size.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(out)
}
fn write_new(path: &Path, data: &[u8]) -> AppResult<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}
#[cfg(test)]
fn save_at(
    root: &Path,
    request: SampleRequest,
    samples: &[i16],
    session: &str,
) -> AppResult<Sample> {
    save_active_at(root, request, samples, session, || true)
}
fn save_active_at(
    root: &Path,
    request: SampleRequest,
    samples: &[i16],
    session: &str,
    can_commit: impl Fn() -> bool,
) -> AppResult<Sample> {
    request.validate()?;
    if !can_commit() {
        return Err(AppError::Cancelled(
            "Запись образца отменена или уже завершена.".into(),
        ));
    }
    if samples.len() > (request.duration_ms() as usize + 2_000) * RATE / 1000 {
        return Err(AppError::Audio(
            "Запись слишком короткая или превышает лимит. Повторите образец.".into(),
        ));
    }
    regular_dir(root)?;
    if load(root)?.len() >= MAX_RECORDINGS {
        return Err(AppError::Config(
            "Достигнут лимит образцов. Удалите ненужные записи.".into(),
        ));
    }
    let identity = root.join("speaker.txt");
    let speaker = if identity.exists() {
        let value = String::from_utf8(regular_file(&identity, 36)?)
            .map_err(|_| AppError::Config("Каталог образцов недоступен.".into()))?;
        identifier(&value)?;
        value
    } else {
        let value = Uuid::new_v4().to_string();
        write_new(&identity, value.as_bytes())?;
        value
    };
    let audio = wav(samples)?;
    let phrase = request.phrase_id.text(&request.language)?.to_owned();
    let sample = Sample {
        id: Uuid::new_v4().to_string(),
        phrase_id: request.phrase_id,
        language: request.language,
        kind: request.kind,
        condition: request.condition,
        duration_ms: (samples.len() * 1000 / RATE) as u64,
        sha256: format!("{:x}", Sha256::digest(&audio)),
        recorded_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };
    let pending = root.join(format!(".{}.pending", sample.id));
    fs::create_dir(&pending)?;
    let recording = Recording {
        schema: "fono-wake-recording@2".into(),
        sample: sample.clone(),
        phrase: Some(phrase),
        source_group: speaker.clone(),
        speaker,
        session: session.into(),
        split: "dev".into(),
        permission: "consented-local-only".into(),
        purpose: "wake-detector-development".into(),
        // Voice presence/energy cannot establish phrase correctness or phrase-end ground truth.
        annotation: None,
    };
    let result = (|| {
        write_new(&pending.join("recording.wav"), &audio)?;
        write_new(
            &pending.join("recording.json"),
            &serde_json::to_vec_pretty(&recording)?,
        )?;
        if !can_commit() {
            return Err(AppError::Cancelled(
                "Запись образца отменена или уже завершена.".into(),
            ));
        }
        fs::rename(&pending, root.join(&sample.id))?;
        Ok(sample)
    })();
    if result.is_err() {
        // Only these freshly-created files belong to this transaction; no recursive deletion.
        let _ = fs::remove_file(pending.join("recording.wav"));
        let _ = fs::remove_file(pending.join("recording.json"));
        let _ = fs::remove_dir(pending);
    }
    result
}
pub(crate) fn save(
    request: SampleRequest,
    samples: &[i16],
    session: &str,
    can_commit: impl Fn() -> bool,
) -> AppResult<Sample> {
    let _guard = STORE.lock();
    save_active_at(&root()?, request, samples, session, can_commit)
}
fn read_audio_entry_at(root: &Path, id: &str) -> AppResult<(Recording, Vec<u8>)> {
    regular_dir(root)?;
    let dir = entry_dir(root, id)?;
    let recording: Recording =
        serde_json::from_slice(&regular_file(&dir.join("recording.json"), 16_384)?)?;
    recording.validate(id)?;
    let audio = regular_file(&dir.join("recording.wav"), (RATE * 44 + 44) as u64)?;
    if recording.sample.sha256 != format!("{:x}", Sha256::digest(&audio)) {
        return Err(AppError::Config("Образец повреждён или недоступен.".into()));
    }
    Ok((recording, audio))
}
fn read_audio_at(root: &Path, id: &str) -> AppResult<String> {
    Ok(STANDARD.encode(read_audio_entry_at(root, id)?.1))
}
pub(crate) fn read_audio(id: &str) -> AppResult<String> {
    let _guard = STORE.lock();
    read_audio_at(&root()?, id)
}
#[cfg(debug_assertions)]
pub(super) fn detector_input(id: &str) -> AppResult<Vec<u8>> {
    let _guard = STORE.lock();
    detector_input_at(&root()?, id)
}
#[cfg(debug_assertions)]
fn detector_input_at(root: &Path, id: &str) -> AppResult<Vec<u8>> {
    let (recording, audio) = read_audio_entry_at(root, id)?;
    if recording.sample.phrase_id != PhraseId::PrivetLira
        || recording.sample.language != Language::Ru
        || recording.permission != "consented-local-only"
        || recording.split != "dev"
    {
        return Err(AppError::Config(
            "Проверка доступна для записей «Привет, Лира».".into(),
        ));
    }
    Ok(audio)
}
fn delete_at(root: &Path, id: &str) -> AppResult<()> {
    regular_dir(root)?;
    identifier(id)?;
    // A renamed tombstone is immediately absent from reads, even if file removal fails.
    let tombstone = root.join(format!(".{}.deleted", id));
    if !tombstone.exists() {
        fs::rename(entry_dir(root, id)?, &tombstone)?;
    }
    let meta = fs::symlink_metadata(&tombstone)?;
    if !meta.is_dir() || meta.file_type().is_symlink() || reparse(&meta) {
        return Err(AppError::Config("Образец повреждён или недоступен.".into()));
    }
    for name in ["recording.wav", "recording.json"] {
        match fs::remove_file(tombstone.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    fs::remove_dir(tombstone)?;
    Ok(())
}
pub(crate) fn delete(id: &str) -> AppResult<()> {
    let _guard = STORE.lock();
    delete_at(&root()?, id)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
