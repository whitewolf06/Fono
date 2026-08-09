//! AI-постобработка транскрипта через локальный LLM-сервер (LM Studio).
//!
//! Использует OpenAI-совместимый API: `POST {base_url}/chat/completions`.
//! Модель может поддерживать function calling — структура `tools` передаётся
//! в запросе, но активные инструменты пока не вызываются (заготовка для
//! будущих команд: «ответь на email», «кратко перескажи» и т.п.).

use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::types::{AiMode, LlmProvider, Settings};

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_TOKENS: u32 = 2048;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

static HTTP_CLIENT: Lazy<Result<reqwest::Client, String>> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|error| error.to_string())
});

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
    api_key: Option<String>,
}

impl LlmClient {
    pub fn new(
        base_url: impl Into<String>,
        model: Option<String>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            model,
            api_key,
        }
    }

    pub fn from_settings(settings: &Settings) -> Self {
        let base_url = match settings.llm_provider {
            LlmProvider::OpenAi if settings.llm_base_url.trim().is_empty() => {
                "https://api.openai.com/v1".to_string()
            }
            _ => settings.llm_base_url.clone(),
        };
        Self {
            base_url,
            model: settings.llm_model.clone(),
            api_key: settings.llm_api_key.clone(),
        }
    }

    /// Проверяет соединение с LLM-сервером — возвращает имя первой доступной модели.
    pub async fn test_connection(&self) -> AppResult<String> {
        let models = self.list_models().await?;
        models
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Llm("нет доступных моделей".into()))
    }

    /// Возвращает список доступных моделей.
    pub async fn list_models(&self) -> AppResult<Vec<String>> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let mut req = shared_http_client()?.get(&url).timeout(CONNECT_TIMEOUT);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Llm(format!("GET {url}: {e}")))?;

        if !resp.status().is_success() {
            return Err(AppError::Llm(format!(
                "LLM сервер ответил {} на GET {url}",
                resp.status()
            )));
        }
        let body: ModelsResponse = decode_json_response(resp, "parse /models").await?;

        Ok(body.data.into_iter().map(|m| m.id).collect())
    }

    /// Применяет выбранную обработку к транскрипту.
    /// Если модель не указана или режим Off — возвращает исходный текст.
    /// `clean_prompt` позволяет пользователю переопределить системный промт для режима Clean.
    pub async fn process(
        &self,
        transcript: &str,
        mode: AiMode,
        clean_prompt: Option<&str>,
    ) -> AppResult<String> {
        if matches!(mode, AiMode::Off) {
            return Ok(transcript.to_string());
        }

        let Some(model) = self.model.as_deref() else {
            tracing::debug!("LLM model not set, returning raw transcript");
            return Ok(transcript.to_string());
        };

        let system = system_prompt(mode, clean_prompt);
        let user = user_prompt(transcript, mode);
        crate::vlog!("LLM request model={} mode={:?}", model, mode);
        crate::vlog!("LLM user prompt prepared ({} chars)", user.chars().count());

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

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req_builder = shared_http_client()?
            .post(&url)
            .timeout(REQUEST_TIMEOUT)
            .json(&req);
        if let Some(key) = &self.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {key}"));
        }
        let resp = req_builder
            .send()
            .await
            .map_err(|e| AppError::Llm(format!("POST {url}: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(AppError::Llm(format!(
                "LLM сервер ответил статусом {status}"
            )));
        }

        let body: ChatResponse = decode_json_response(resp, "parse response").await?;
        let result = response_content(body)?;
        crate::vlog!("LLM response received ({} chars)", result.chars().count());
        Ok(result)
    }
}

fn shared_http_client() -> AppResult<&'static reqwest::Client> {
    HTTP_CLIENT
        .as_ref()
        .map_err(|error| AppError::Llm(format!("initialize HTTP client: {error}")))
}

async fn decode_json_response<T>(mut response: reqwest::Response, context: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AppError::Llm(format!("{context}: {error}")))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(AppError::Llm(format!(
                "{context}: LLM response exceeds {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|error| AppError::Llm(format!("{context}: {error}")))
}

fn response_content(response: ChatResponse) -> AppResult<String> {
    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content.trim().to_string())
        .filter(|content| !content.is_empty())
        .ok_or_else(|| AppError::Llm("empty LLM response".into()))
}

fn system_prompt(mode: AiMode, clean_prompt: Option<&str>) -> String {
    match mode {
        AiMode::Off => String::new(),
        AiMode::Clean => {
            if let Some(prompt) = clean_prompt {
                if !prompt.trim().is_empty() {
                    return prompt.trim().to_string();
                }
            }
            r#"Ты — редактор голосовых транскриптов.
Задача: превратить сырой распознанный текст в читаемый, не меняя смысл.

Правила:
1. Удали слова-паразиты и запинки: «ээ», «мм», «ну», «типа», «короче», «как бы», «значит», «вот» и им подобные.
2. Исправь явные оговорки и повторы, если они мешают чтению.
3. Поставь пунктуацию и заглавные буквы в начале предложений.
4. Сохрани язык оригинала и стиль говорящего (формальный/неформальный).
5. НЕ добавляй пояснений, приветствий и прощаний.
6. Верни ТОЛЬКО готовый текст."#.to_string()
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn response_with(content: &str) -> ChatResponse {
        ChatResponse {
            choices: vec![ChatChoice {
                message: ChatChoiceMessage {
                    content: content.to_string(),
                },
            }],
        }
    }

    #[test]
    fn response_content_trims_valid_content() {
        assert_eq!(
            response_content(response_with("  result  ")).unwrap(),
            "result"
        );
    }

    #[test]
    fn response_content_rejects_empty_content() {
        assert!(response_content(response_with(" \n ")).is_err());
        assert!(response_content(ChatResponse { choices: vec![] }).is_err());
    }

    #[test]
    fn http_client_is_shared_between_requests() {
        let first = shared_http_client().expect("initialize shared client");
        let second = shared_http_client().expect("reuse shared client");
        assert!(std::ptr::eq(first, second));
    }
}
