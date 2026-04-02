use crate::{ModelProvider, Request, Response, ResponseChunk};
use async_trait::async_trait;
use futures_util::{stream::BoxStream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest_eventsource::{Event, EventSource};

/// Native Anthropic Provider.
pub struct AnthropicProvider {
    pub api_key: String,
    pub model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string()),
        }
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    async fn complete(&self, _req: Request) -> anyhow::Result<Response> {
        anyhow::bail!("Non-streaming complete not implemented yet for live providers. Please use stream_complete.")
    }

    async fn stream_complete(
        &self,
        req: Request,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        let client = reqwest::Client::new();
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key)?);
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));

        let payload = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": req.prompt}],
            "stream": true
        });

        let mut source = EventSource::new(
            client
                .post("https://api.anthropic.com/v1/messages")
                .headers(headers)
                .json(&payload),
        )?;

        let stream = async_stream::try_stream! {
            while let Some(event) = source.next().await {
                match event {
                    Ok(Event::Message(message)) => {
                        let data: serde_json::Value = serde_json::from_str(&message.data)?;
                        if let Some(delta) = data.get("delta").and_then(|d| d.get("text")).and_then(|t| t.as_str()) {
                            yield ResponseChunk {
                                content_delta: Some(delta.to_string()),
                                tool_call_delta: None,
                                is_final: false,
                            };
                        }
                        if data.get("type").and_then(|t| t.as_str()) == Some("message_stop") {
                            yield ResponseChunk {
                                content_delta: None,
                                tool_call_delta: None,
                                is_final: true,
                            };
                        }
                    }
                    Err(e) => {
                        source.close();
                        Err(anyhow::anyhow!("Anthropic stream error: {}", e))?;
                    }
                    _ => {}
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

/// Native OpenAI Provider (also supports Ollama).
pub struct OpenAiProvider {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "gpt-4o".to_string()),
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }

    pub fn ollama(model: String) -> Self {
        Self {
            api_key: "ollama".to_string(),
            model,
            base_url: "http://localhost:11434/v1".to_string(),
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    async fn complete(&self, _req: Request) -> anyhow::Result<Response> {
        anyhow::bail!("Non-streaming complete not implemented yet for live providers. Please use stream_complete.")
    }

    async fn stream_complete(
        &self,
        req: Request,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": req.prompt}],
            "stream": true
        });

        let mut source = EventSource::new(
            client
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(&self.api_key)
                .json(&payload),
        )?;

        let stream = async_stream::try_stream! {
            while let Some(event) = source.next().await {
                match event {
                    Ok(Event::Message(message)) => {
                        if message.data == "[DONE]" {
                            yield ResponseChunk { content_delta: None, tool_call_delta: None, is_final: true };
                            break;
                        }
                        let data: serde_json::Value = serde_json::from_str(&message.data)?;
                        if let Some(delta) = data.get("choices").and_then(|c| c[0].get("delta")).and_then(|d| d.get("content")).and_then(|t| t.as_str()) {
                            yield ResponseChunk {
                                content_delta: Some(delta.to_string()),
                                tool_call_delta: None,
                                is_final: false,
                            };
                        }
                    }
                    Err(e) => {
                        source.close();
                        Err(anyhow::anyhow!("OpenAI stream error: {}", e))?;
                    }
                    _ => {}
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

/// Native Google Gemini Provider.
pub struct GeminiProvider {
    pub api_key: String,
    pub model: String,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "gemini-1.5-flash".to_string()),
        }
    }
}

#[async_trait]
impl ModelProvider for GeminiProvider {
    async fn complete(&self, _req: Request) -> anyhow::Result<Response> {
        anyhow::bail!("Non-streaming complete not implemented yet for live providers. Please use stream_complete.")
    }

    async fn stream_complete(
        &self,
        req: Request,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        let client = reqwest::Client::new();
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?key={}",
            self.model, self.api_key
        );

        let payload = serde_json::json!({
            "contents": [{"parts": [{"text": req.prompt}]}]
        });

        // Gemini's streamGenerateContent returns a JSON array of objects over time.
        let response = client.post(url).json(&payload).send().await?;
        let mut stream = response.bytes_stream();

        let stream = async_stream::try_stream! {
            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(|e| anyhow::anyhow!(e))?;
                let text = String::from_utf8_lossy(&bytes);
                let data: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
                if let Some(candidate) = data.get("candidates").and_then(|c| c[0].get("content")).and_then(|ct| ct.get("parts")).and_then(|p| p[0].get("text")).and_then(|t| t.as_str()) {
                    yield ResponseChunk {
                        content_delta: Some(candidate.to_string()),
                        tool_call_delta: None,
                        is_final: false,
                    };
                }
            }
            yield ResponseChunk { content_delta: None, tool_call_delta: None, is_final: true };
        };

        Ok(Box::pin(stream))
    }
}
