//! AI-постобработка транскрипта через локальный LLM-сервер (LM Studio).
//!
//! Использует OpenAI-совместимый API: `POST {base_url}/chat/completions`.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::types::AiMode;

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

pub struct LlmClient {
    base_url: String,
    model: Option<String>,
}

impl LlmClient {
    pub fn new(base_url: impl Into<String>, model: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model,
        }
    }

    /// Проверяет соединение с LM Studio — возвращает имя активной модели.
    pub async fn test_connection(&self) -> AppResult<String> {
        let client = reqwest::Client::builder()
            .timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| AppError::Llm(e.to_string()))?;

        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Llm(format!("GET {url}: {e}")))?;

        if !resp.status().is_success() {
            return Err(AppError::Llm(format!(
                "LM Studio ответил {} на GET {url}",
                resp.status()
            )));
        }
        let body: ModelsResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Llm(format!("parse /models: {e}")))?;

        body.data
            .into_iter()
            .next()
            .map(|m| m.id)
            .ok_or_else(|| AppError::Llm("модель не загружена в LM Studio".into()))
    }

    /// Применяет выбранную обработку к транскрипту.
    pub async fn process(&self, transcript: &str, mode: AiMode) -> AppResult<String> {
        if matches!(mode, AiMode::Off) {
            return Ok(transcript.to_string());
        }

        let model = self
            .model
            .as_deref()
            .ok_or_else(|| AppError::Llm("модель LLM не указана в настройках".into()))?;

        let system = system_prompt(mode);
        let user = user_prompt(transcript, mode);

        let req = ChatRequest {
            model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: &system,
                },
                ChatMessage {
                    role: "user",
                    content: &user,
                },
            ],
            temperature: 0.2,
            stream: false,
        };

        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| AppError::Llm(e.to_string()))?;

        let url = format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let resp = client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| AppError::Llm(format!("POST {url}: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Llm(format!(
                "LM Studio ответил {status}: {text}"
            )));
        }

        let body: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Llm(format!("parse response: {e}")))?;

        body.choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .ok_or_else(|| AppError::Llm("пустой ответ LLM".into()))
    }
}

fn system_prompt(mode: AiMode) -> String {
    match mode {
        AiMode::Off => String::new(),
        AiMode::Clean => r#"Ты — ассистент, который чистит голосовые транскрипты.
Правила:
- Убери слова-паразиты, запинки, оговорки («ээ», «мм», «ну», «типа», «короче»).
- Добавь пунктуацию и заглавные буквы.
- НЕ меняй смысл и стиль говорящего.
- Сохрани язык оригинала.
- Верни ТОЛЬКО очищенный текст, без пояснений."#.to_string(),
        AiMode::Format => r#"Ты — ассистент, который форматирует голосовые транскрипты.
Правила:
- Убери слова-паразиты и оговорки, добавь пунктуацию.
- Разбей на абзацы / списки там, где это уместно.
- НЕ меняй смысл, стиль и язык оригинала.
- Верни ТОЛЬКО отформатированный текст."#.to_string(),
        AiMode::Command => r#"Ты — ассистент голосового ввода.
Пользователь продиктует команду и контент. Выполни команду (например: «переведи на английский», «преврати в email», «сделай короче», «исправь ошибки»).
- Сохрани смысл, если команда не указана явно — просто почисти текст.
- Верни ТОЛЬКО результат, без пояснений."#.to_string(),
    }
}

fn user_prompt(transcript: &str, _mode: AiMode) -> String {
    // Транскрипт передаём как есть. LLM получает уже сам текст.
    transcript.to_string()
}
