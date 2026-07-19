//! Small, provider-neutral boundary for Kilo Gateway chat completions.
//!
//! The rest of Ilium depends only on `KiloGatewayClient::complete_text`:
//! it supplies a model and messages, receives an assistant string, and never
//! needs to know about Kilo's HTTP endpoint, retryable status codes, or the
//! OpenAI-compatible response envelope.

use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Kilo's documented OpenAI-compatible base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.kilo.ai/api/gateway";
/// The no-credential model selected for Ilium's lightweight metadata jobs.
pub const DEFAULT_FREE_MODEL: &str = "openrouter/free";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One text-only OpenAI-compatible chat message.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system",
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user",
            content: content.into(),
        }
    }
}

/// Inputs that affect one completion request, independent from the HTTP client.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl CompletionRequest {
    pub fn with_default_free_model(messages: Vec<ChatMessage>) -> Self {
        Self {
            model: DEFAULT_FREE_MODEL.to_string(),
            messages,
            // Free routers can select reasoning-capable models. 256 was not
            // enough headroom in practice: verified live against a real
            // Codex session-title prompt (~3.4k chars of structured task
            // instructions) routed through `kilo-auto/free` to
            // `tencent/hy3-.../free`, which spent its entire budget on
            // invisible reasoning tokens and returned `finish_reason:
            // "length"` with a null visible `content` -- surfaced as
            // `GatewayError::InvalidResponse("missing non-empty assistant
            // content")`, not a parse failure. The same prompt succeeded at
            // 1024; 1536 keeps margin for longer or more structured inputs
            // (a full agentic task prompt, not just a short human-typed
            // one) without being needlessly large for the tiny JSON reply
            // this is actually asking for. `openrouter/free` is also a
            // dynamic router, so the same headroom applies.
            max_tokens: 1536,
            temperature: 0.0,
        }
    }
}

/// Retry policy for temporary gateway or upstream failures.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub initial_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
        }
    }
}

/// Failure information stable enough for UI workflows to decide whether to retry later.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("Kilo Gateway returned HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("could not send request to Kilo Gateway: {0}")]
    Transport(String),
    #[error("Kilo Gateway returned an invalid response: {0}")]
    InvalidResponse(String),
}

impl GatewayError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Transport(_))
            || matches!(
                self,
                Self::Http {
                    status: 429 | 500 | 502 | 503 | 504,
                    ..
                }
            )
    }
}

/// Client for Kilo Gateway's OpenAI-compatible `/chat/completions` endpoint.
pub struct KiloGatewayClient {
    base_url: String,
    retry_policy: RetryPolicy,
}

impl Default for KiloGatewayClient {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL, RetryPolicy::default())
    }
}

impl KiloGatewayClient {
    pub fn new(base_url: impl Into<String>, retry_policy: RetryPolicy) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            retry_policy,
        }
    }

    /// Sends a non-streaming completion and returns only its assistant text.
    ///
    /// This intentionally omits `Authorization`: Kilo identifies anonymous
    /// `openrouter/free` calls by public IP, which is the requested default.
    pub fn complete_text(&self, request: &CompletionRequest) -> Result<String, GatewayError> {
        self.complete_text_with_sender(request, |payload| self.send_once(payload))
    }

    fn complete_text_with_sender<F>(
        &self,
        request: &CompletionRequest,
        mut send: F,
    ) -> Result<String, GatewayError>
    where
        F: FnMut(&ChatCompletionPayload) -> Result<ChatCompletionResponse, GatewayError>,
    {
        let payload = ChatCompletionPayload {
            model: &request.model,
            messages: &request.messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stream: false,
        };
        let attempts = self.retry_policy.max_attempts.max(1);
        let mut delay = self.retry_policy.initial_delay;

        for attempt in 1..=attempts {
            tracing::info!(
                provider = "kilo_gateway",
                attempt,
                attempts,
                request_body = %serde_json::to_string(&payload).unwrap_or_else(|error| format!("<serialization failed: {error}>")),
                "LLM provider attempt started"
            );
            match send(&payload) {
                Ok(response) => {
                    let result = response.assistant_text();
                    match &result {
                        Ok(text) => {
                            tracing::info!(provider = "kilo_gateway", attempt, response_text = %text, "LLM provider attempt completed")
                        }
                        Err(error) => {
                            tracing::error!(provider = "kilo_gateway", attempt, error = %error, "LLM provider response validation failed")
                        }
                    }
                    return result;
                }
                Err(error) if error.is_retryable() && attempt < attempts => {
                    tracing::warn!(
                        provider = "kilo_gateway",
                        attempt,
                        error = %error,
                        error_debug = ?error,
                        retry_delay_milliseconds = delay.as_millis(),
                        "LLM provider attempt failed and will retry"
                    );
                    thread::sleep(delay);
                    delay = delay.saturating_mul(2).min(self.retry_policy.max_delay);
                }
                Err(error) => {
                    tracing::error!(provider = "kilo_gateway", attempt, error = %error, error_debug = ?error, "LLM provider attempt failed");
                    return Err(error);
                }
            }
        }
        unreachable!("the non-empty retry loop always returns")
    }

    fn send_once(
        &self,
        payload: &ChatCompletionPayload<'_>,
    ) -> Result<ChatCompletionResponse, GatewayError> {
        let url = format!("{}/chat/completions", self.base_url);
        let diagnostic_url = ilium_logging::redacted_url(&url);
        tracing::info!(
            method = "POST",
            url = %diagnostic_url,
            headers = ?[("Content-Type", "application/json")],
            request_body = %serde_json::to_string(payload).unwrap_or_else(|error| format!("<serialization failed: {error}>")),
            "HTTP request started"
        );
        // A one-shot UI enrichment must never leave its tracked worker
        // waiting indefinitely on a broken network path.
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(REQUEST_TIMEOUT))
                .http_status_as_error(false)
                .build(),
        );
        let mut response = match agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send_json(payload)
        {
            Ok(response) => response,
            Err(error) => {
                let error_message = error.to_string().replace(&url, &diagnostic_url);
                tracing::error!(method = "POST", url = %diagnostic_url, error = %error_message, "HTTP transport failed");
                return Err(GatewayError::Transport(error_message));
            }
        };
        let status = response.status().as_u16();
        let response_headers = redacted_response_headers(response.headers());
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| {
                tracing::error!(method = "POST", url = %diagnostic_url, status, ?response_headers, error = %error, "failed to read HTTP response body");
                GatewayError::Transport(error.to_string())
            })?;
        if !(200..300).contains(&status) {
            tracing::error!(method = "POST", url = %diagnostic_url, status, ?response_headers, response_body = %body, "HTTP request failed");
            return Err(GatewayError::Http {
                status,
                message: body,
            });
        }
        tracing::info!(method = "POST", url = %diagnostic_url, status, ?response_headers, response_body = %body, "HTTP request completed");
        serde_json::from_str(&body).map_err(|error| {
            tracing::error!(
                method = "POST",
                url = %diagnostic_url,
                status,
                ?response_headers,
                response_body = %body,
                error = %error,
                "HTTP response was not valid JSON"
            );
            GatewayError::InvalidResponse(error.to_string())
        })
    }
}

fn redacted_response_headers(headers: &ureq::http::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = value.to_str().unwrap_or("<non-text header>");
            (
                name.as_str().to_owned(),
                ilium_logging::redacted_header_value(name.as_str(), value),
            )
        })
        .collect()
}

#[derive(Serialize)]
struct ChatCompletionPayload<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    max_tokens: u32,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

impl ChatCompletionResponse {
    fn assistant_text(self) -> Result<String, GatewayError> {
        self.choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            // Trim before the emptiness check, not after: callers receive
            // exactly the string that was validated as non-empty, so a
            // whitespace-only reply can't slip through as e.g. a single
            // trailing newline.
            .map(|content| content.trim().to_string())
            .filter(|content| !content.is_empty())
            .ok_or_else(|| {
                GatewayError::InvalidResponse("missing non-empty assistant content".to_string())
            })
    }
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    fn spawn_http_response(status: &str, body: &str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP fixture");
        let address = listener.local_addr().expect("fixture address");
        let status = status.to_owned();
        let body = body.to_owned();
        let (request_sender, request_receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local HTTP request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("request timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4_096];
            loop {
                let read = stream.read(&mut buffer).expect("read HTTP request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            request_sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .expect("capture HTTP request");
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write HTTP response");
        });
        (format!("http://{address}"), request_receiver)
    }

    fn request() -> CompletionRequest {
        CompletionRequest::with_default_free_model(vec![ChatMessage::user("name this project")])
    }

    #[test]
    fn uses_openrouter_free_by_default() {
        assert_eq!(request().model, DEFAULT_FREE_MODEL);
    }

    #[test]
    fn retries_temporary_failures_then_returns_assistant_text() {
        let client = KiloGatewayClient::new(
            "https://example.invalid",
            RetryPolicy {
                max_attempts: 3,
                initial_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        );
        let calls = Cell::new(0);

        let result = client.complete_text_with_sender(&request(), |_| {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                return Err(GatewayError::Http {
                    status: 429,
                    message: "slow down".to_string(),
                });
            }
            Ok(ChatCompletionResponse {
                choices: vec![ChatChoice {
                    message: ChatChoiceMessage {
                        content: Some("Ilium".to_string()),
                    },
                }],
            })
        });

        assert_eq!(result.unwrap(), "Ilium");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn does_not_retry_invalid_responses() {
        let client = KiloGatewayClient::new(
            "https://example.invalid",
            RetryPolicy {
                max_attempts: 3,
                initial_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        );
        let calls = Cell::new(0);
        let result = client.complete_text_with_sender(&request(), |_| {
            calls.set(calls.get() + 1);
            Err(GatewayError::InvalidResponse("not JSON".to_string()))
        });

        assert!(matches!(result, Err(GatewayError::InvalidResponse(_))));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn http_errors_preserve_the_complete_gateway_body_and_request_prompt() {
        let response_body =
            r#"{"error":{"message":"upstream rejected request","request_id":"req-42"}}"#;
        let (base_url, request_receiver) = spawn_http_response("502 Bad Gateway", response_body);
        let client = KiloGatewayClient::new(
            base_url,
            RetryPolicy {
                max_attempts: 1,
                initial_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        );

        let result = client.complete_text(&request());

        assert!(matches!(
            result,
            Err(GatewayError::Http { status: 502, message }) if message == response_body
        ));
        let captured_request = request_receiver.recv().expect("captured HTTP request");
        assert!(captured_request.contains("name this project"));
        assert!(captured_request.contains("openrouter/free"));
    }
}
