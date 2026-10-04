//! AI-постобработка транскрипта через локальный LLM-сервер (LM Studio).
//!
//! Использует OpenAI-совместимый API: `POST {base_url}/chat/completions`.
//! Модель может поддерживать function calling — структура `tools` передаётся
//! в запросе, но активные инструменты пока не вызываются (заготовка для
//! будущих команд: «ответь на email», «кратко перескажи» и т.п.).

mod presets;

use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::types::{
    AiMode, LlmProfile, LlmProvider, Settings, SpeechFinding, SpeechSessionAnalysis,
};

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
        if let Some(profile) = settings.correction_profile() {
            return Self::from_profile(profile, settings.text_correction_llm.model.as_deref());
        }
        let base_url = match settings.llm_provider {
            LlmProvider::OpenAi if settings.llm_base_url.trim().is_empty() => {
                "https://api.openai.com/v1".to_string()
            }
            _ => settings.llm_base_url.clone(),
        };
        let api_key = match crate::secrets::load_llm_api_key() {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(%error, "could not load LLM API key from secure storage");
                None
            }
        };
        Self {
            base_url,
            model: settings.llm_model.clone(),
            api_key,
        }
    }

    pub fn from_profile(profile: &LlmProfile, model_override: Option<&str>) -> Self {
        let base_url = match profile.provider {
            LlmProvider::OpenAi if profile.base_url.trim().is_empty() => {
                "https://api.openai.com/v1".to_string()
            }
            _ => profile.base_url.clone(),
        };
        let api_key = match crate::secrets::load_llm_profile_api_key(&profile.id) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(%error, profile_id = profile.id, "could not load LLM API key from secure storage");
                None
            }
        };
        Self {
            base_url,
            model: model_override
                .filter(|model| !model.trim().is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| profile.model.clone()),
            api_key,
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

        self.chat(model, &system, &user, MAX_TOKENS).await
    }

    pub async fn analyze_speech(
        &self,
        analysis: &SpeechSessionAnalysis,
        findings: Option<&[SpeechFinding]>,
        original_text: Option<&str>,
    ) -> AppResult<SpeechLlmRecommendation> {
        let Some(model) = self.model.as_deref() else {
            return Err(AppError::Llm(
                "для LLM-анализа речи не выбрана модель".into(),
            ));
        };
        let user = serde_json::to_string(&SpeechAnalysisRequest {
            metrics: analysis,
            findings,
            original_text,
        })
        .map_err(|error| AppError::Llm(format!("serialize speech analysis input: {error}")))?;
        let content = self
            .chat(model, SPEECH_ANALYSIS_SYSTEM_PROMPT, &user, 900)
            .await?;
        parse_speech_recommendation(&content, analysis.findings.len())
    }

    async fn chat(
        &self,
        model: &str,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> AppResult<String> {
        let req = ChatRequest {
            model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: system,
                },
                ChatMessage {
                    role: "user",
                    content: user,
                },
            ],
            temperature: 0.2,
            stream: false,
            max_tokens,
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

#[derive(Debug, Serialize)]
struct SpeechAnalysisRequest<'a> {
    metrics: &'a SpeechSessionAnalysis,
    #[serde(skip_serializing_if = "Option::is_none")]
    findings: Option<&'a [SpeechFinding]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_text: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechLlmRecommendation {
    pub summary: String,
    pub recommendations: Vec<SpeechLlmRecommendationItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechLlmRecommendationItem {
    pub title: String,
    pub observation: String,
    pub exercise: String,
    #[serde(default)]
    pub finding_indexes: Vec<usize>,
}

const SPEECH_ANALYSIS_SYSTEM_PROMPT: &str = r#"Ты — бережный тренер речи. На входе уже есть детерминированные локальные метрики.
Они являются фактом; не пересчитывай и не выдумывай проблемы. Дай короткое практичное резюме и не более трёх рекомендаций.
Верни только JSON без Markdown: {"summary":"...","recommendations":[{"title":"...","observation":"...","exercise":"...","finding_indexes":[0]}]}.
Индексы могут ссылаться только на переданные findings."#;

fn parse_speech_recommendation(
    content: &str,
    finding_count: usize,
) -> AppResult<SpeechLlmRecommendation> {
    let content = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let mut recommendation: SpeechLlmRecommendation =
        serde_json::from_str(content).map_err(|error| {
            AppError::Llm(format!(
                "LLM returned invalid speech recommendation JSON: {error}"
            ))
        })?;
    recommendation.summary = recommendation.summary.trim().chars().take(500).collect();
    recommendation.recommendations.truncate(3);
    if recommendation.summary.is_empty() {
        return Err(AppError::Llm(
            "LLM returned an empty speech recommendation".into(),
        ));
    }
    for item in &mut recommendation.recommendations {
        item.title = item.title.trim().chars().take(120).collect();
        item.observation = item.observation.trim().chars().take(500).collect();
        item.exercise = item.exercise.trim().chars().take(500).collect();
        if item.title.is_empty() || item.observation.is_empty() || item.exercise.is_empty() {
            return Err(AppError::Llm(
                "LLM returned an incomplete speech recommendation".into(),
            ));
        }
        item.finding_indexes.retain(|index| *index < finding_count);
    }
    Ok(recommendation)
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn response_with(content: &str) -> ChatResponse {
        ChatResponse {
            choices: vec![ChatChoice {
                message: ChatChoiceMessage {
                    content: content.to_string(),
                },
            }],
        }
    }

    fn test_server(status: &str, content_type: &str, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let address = listener
            .local_addr()
            .expect("read local test server address");
        let status = status.to_string();
        let content_type = content_type.to_string();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept one client");
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut request = [0_u8; 1_024];
            let _ = stream.read(&mut request);
            let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(headers.as_bytes())
                .and_then(|_| stream.write_all(&body))
                .expect("write local test response");
        });

        format!("http://{address}/v1")
    }

    fn run_async<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create test runtime")
            .block_on(future)
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

    #[test]
    fn http_fault_injection_preserves_non_success_status() {
        let base_url = test_server(
            "503 Service Unavailable",
            "application/json",
            b"{}".to_vec(),
        );
        let client = LlmClient::new(base_url, Some("test-model".to_string()), None);

        let error = run_async(client.list_models()).expect_err("503 must fail");
        assert!(error.to_string().contains("503 Service Unavailable"));
    }

    #[test]
    fn http_fault_injection_rejects_malformed_json() {
        let base_url = test_server("200 OK", "application/json", b"not-json".to_vec());
        let client = LlmClient::new(base_url, Some("test-model".to_string()), None);

        let error = run_async(client.list_models()).expect_err("malformed JSON must fail");
        assert!(error.to_string().contains("parse /models"));
    }

    #[test]
    fn http_fault_injection_rejects_oversized_response() {
        let base_url = test_server(
            "200 OK",
            "application/json",
            vec![b'x'; MAX_RESPONSE_BYTES + 1],
        );
        let client = LlmClient::new(base_url, Some("test-model".to_string()), None);

        let error = run_async(client.list_models()).expect_err("oversized response must fail");
        assert!(error.to_string().contains("response exceeds"));
    }

    #[test]
    fn speech_recommendation_keeps_only_known_finding_indexes() {
        let recommendation = parse_speech_recommendation(
            r#"{"summary":"Коротко","recommendations":[{"title":"Пауза","observation":"Есть вводные","exercise":"Сделайте паузу","finding_indexes":[0,4]}]}"#,
            1,
        )
        .expect("valid response");

        assert_eq!(recommendation.recommendations[0].finding_indexes, vec![0]);
    }
}
