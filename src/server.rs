use std::time::Duration;

use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct OcrClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct OcrRequest<'a> {
    image_base64: &'a str,
    mime_type: &'a str,
    page: u32,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: Vec<ChatContent<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ChatContent<'a> {
    #[serde(rename = "text")]
    Text { text: &'a str },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl<'a> },
}

#[derive(Debug, Serialize)]
struct ImageUrl<'a> {
    url: &'a str,
}

impl OcrClient {
    pub fn new(base_url: String) -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(120))
            .build()?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    pub async fn health_check(&self) -> Result<(), AppError> {
        let model_list = format!("{}/models", self.base_url);
        let legacy_health = format!("{}/health", self.base_url);
        let fallback = self.base_url.clone();

        if self.check_url(&model_list).await
            || self.check_url(&legacy_health).await
            || self.check_url(&fallback).await
        {
            Ok(())
        } else {
            Err(AppError::ServerUnreachable(self.base_url.clone()))
        }
    }

    pub async fn ocr_markdown(
        &self,
        model: &str,
        page: u32,
        image_base64_webp: &str,
        image_base64_png: &str,
    ) -> Result<String, AppError> {
        if self.base_url.ends_with("/v1") {
            return self
                .ocr_markdown_openai(model, page, image_base64_png, "image/png")
                .await;
        }

        self.ocr_markdown_legacy(page, image_base64_webp).await
    }

    async fn ocr_markdown_openai(
        &self,
        model: &str,
        page: u32,
        image_base64: &str,
        mime_type: &str,
    ) -> Result<String, AppError> {
        let url = format!("{}/chat/completions", self.base_url);
        let data_url = format!("data:{mime_type};base64,{image_base64}");
        let prompt = format!("<|grounding|>Convert page {page} to markdown.");
        let response = self
            .client
            .post(url)
            .json(&ChatCompletionRequest {
                model,
                messages: vec![ChatMessage {
                    role: "user",
                    content: vec![
                        ChatContent::Text { text: &prompt },
                        ChatContent::ImageUrl {
                            image_url: ImageUrl { url: &data_url },
                        },
                    ],
                }],
            })
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    AppError::Timeout(page)
                } else {
                    AppError::InferenceError(error.to_string())
                }
            })?;

        let status = response.status();
        let payload = response.text().await.map_err(|error| {
            if error.is_timeout() {
                AppError::Timeout(page)
            } else {
                AppError::InferenceError(error.to_string())
            }
        })?;

        if !status.is_success() {
            return Err(AppError::InferenceError(format!(
                "Seite {page}: HTTP {status} - {payload}"
            )));
        }

        let value: Value = serde_json::from_str(&payload)?;
        extract_markdown(&value).ok_or_else(|| {
            AppError::InferenceError(format!(
                "Seite {page}: OCR-Antwort enthaelt kein Markdown/Text-Feld"
            ))
        })
    }

    async fn ocr_markdown_legacy(&self, page: u32, image_base64: &str) -> Result<String, AppError> {
        let url = format!("{}/ocr", self.base_url);
        let response = self
            .client
            .post(url)
            .json(&OcrRequest {
                image_base64,
                mime_type: "image/webp",
                page,
            })
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    AppError::Timeout(page)
                } else {
                    AppError::InferenceError(error.to_string())
                }
            })?;

        let status = response.status();
        let payload = response.text().await.map_err(|error| {
            if error.is_timeout() {
                AppError::Timeout(page)
            } else {
                AppError::InferenceError(error.to_string())
            }
        })?;

        if !status.is_success() {
            return Err(AppError::InferenceError(format!(
                "Seite {page}: HTTP {status} - {payload}"
            )));
        }

        let value: Value = serde_json::from_str(&payload)?;
        extract_markdown(&value).ok_or_else(|| {
            AppError::InferenceError(format!(
                "Seite {page}: OCR-Antwort enthaelt kein Markdown/Text-Feld"
            ))
        })
    }

    async fn check_url(&self, url: &str) -> bool {
        match self.client.get(url).send().await {
            Ok(response) => {
                response.status().is_success() || response.status() == StatusCode::NOT_FOUND
            }
            Err(_) => false,
        }
    }
}

fn extract_markdown(value: &Value) -> Option<String> {
    if let Some(content) = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
    {
        if let Some(text) = content.as_str() {
            return Some(text.to_string());
        }

        if let Some(items) = content.as_array() {
            let text = items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(Value::as_str) == Some("text") {
                        item.get("text").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }

    [
        value.get("markdown"),
        value.get("text"),
        value.get("content"),
        value.get("data").and_then(|data| data.get("markdown")),
        value.get("data").and_then(|data| data.get("text")),
        value.get("result").and_then(|data| data.get("markdown")),
        value.get("result").and_then(|data| data.get("text")),
    ]
    .into_iter()
    .flatten()
    .find_map(|candidate| candidate.as_str().map(ToOwned::to_owned))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::extract_markdown;

    #[test]
    fn extracts_openai_chat_completion_content() {
        let response = json!({
            "choices": [
                {
                    "message": {
                        "content": "# Heading\n\nBody"
                    }
                }
            ]
        });

        assert_eq!(
            extract_markdown(&response).as_deref(),
            Some("# Heading\n\nBody")
        );
    }

    #[test]
    fn extracts_openai_chat_completion_content_parts() {
        let response = json!({
            "choices": [
                {
                    "message": {
                        "content": [
                            { "type": "text", "text": "# Heading" },
                            { "type": "text", "text": "Body" }
                        ]
                    }
                }
            ]
        });

        assert_eq!(
            extract_markdown(&response).as_deref(),
            Some("# Heading\nBody")
        );
    }
}
