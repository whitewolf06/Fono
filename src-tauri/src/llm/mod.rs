//! AI-постобработка транскрипта через локальный LLM-сервер (LM Studio).
//!
//! Использует OpenAI-совместимый API: `POST {base_url}/chat/completions`.
//! Модель может поддерживать function calling — структура `tools` передаётся
//! в запросе, но активные инструменты пока не вызываются (заготовка для
//! будущих команд: «ответь на email», «кратко перескажи» и т.п.).

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::types::AiMode;

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_TOKENS: u32 = 2048;

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    stream: bool,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Tool<'a>>,
}

/// Function-calling tool declaration (OpenAI-compatible).
/// Пока передаётся пустым списком — явно сообщаем модели, что инструменты
/// поддерживаются, но не вызываем их на стороне приложения.
#[derive(Debug, Serialize)]
struct Tool<'a> {
    r#type: &'a str,
    function: Function<'a>,
}

#[derive(Debug, Serialize)]
struct Function<'a> {
    name: &'a str,
    description: &'a str,
    parameters: serde_json::Value,
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
        let models = self.list_models().await?;
        models
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Llm("модель не загружена в LM Studio".into()))
    }

    /// Возвращает список доступных моделей из LM Studio.
    pub async fn list_models(&self) -> AppResult<Vec<String>> {
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

        Ok(body.data.into_iter().map(|m| m.id).collect())
    }

    /// Применяет выбранную обработку к транскрипту.
    /// Если модель не указана или режим Off — возвращает исходный текст.
    pub async fn process(&self, transcript: &str, mode: AiMode) -> AppResult<String> {
        if matches!(mode, AiMode::Off) {
            return Ok(transcript.to_string());
        }

        let Some(model) = self.model.as_deref() else {
            tracing::debug!("LLM model not set, returning raw transcript");
            return Ok(transcript.to_string());
        };

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
            max_tokens: MAX_TOKENS,
            tools: available_tools(),
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
        AiMode::Clean => r#"Ты — редактор голосовых транскриптов.
Задача: превратить сырой распознанный текст в читаемый, не меняя смысл.

Правила:
1. Удали слова-паразиты и запинки: «ээ», «мм», «ну», «типа», «короче», «как бы», «значит», «вот» и им подобные.
2. Исправь явные оговорки и повторы, если они мешают чтению.
3. Поставь пунктуацию и заглавные буквы в начале предложений.
4. Сохрани язык оригинала и стиль говорящего (формальный/неформальный).
5. НЕ добавляй пояснений, приветствий и прощаний.
6. Верни ТОЛЬКО готовый текст."#.to_string(),
        AiMode::Format => r#"Ты — редактор голосовых транскриптов.
Задача: отформатировать сырой распознанный текст, сохранив смысл.

Правила:
1. Удали слова-паразиты и запинки.
2. Добавь пунктуацию и заглавные буквы.
3. Разбей текст на логичные абзацы. Если смысл подразумевает список — оформи его маркированным или нумерованным списком.
4. Сохрани язык оригинала и стиль.
5. НЕ добавляй пояснений.
6. Верни ТОЛЬКО отформатированный текст."#.to_string(),
        AiMode::Command => r#"Ты — ассистент голосового ввода.
Пользователь продиктовал текст, который может начинаться с команды.

Поддерживаемые команды:
- «переведи на <язык>» — переведи весь текст на указанный язык.
- «преврати в email / письмо» — оформи как деловое письмо с приветствием и прощанием.
- «сделай короче / кратко» — сократи, оставив только суть.
- «исправь ошибки» — исправь грамматику и стиль.
- «оформи списком» — преврати в маркированный список.

Если команда не указана явно — просто почисти текст.

Правила:
1. Выполни команду, если она есть.
2. Сохрани смысл и факты.
3. НЕ добавляй пояснений вроде «Вот результат:».
4. Верни ТОЛЬКО результат."#.to_string(),
    }
}

/// Заготовка function-calling инструментов.
/// Пока активные инструменты не вызываются, но поле `tools` передаётся,
/// чтобы модели с поддержкой function calling использовали правильный формат.
fn available_tools<'a>() -> Vec<Tool<'a>> {
    vec![]
}

fn user_prompt(transcript: &str, _mode: AiMode) -> String {
    // Транскрипт передаём как есть. LLM получает уже сам текст.
    transcript.to_string()
}
