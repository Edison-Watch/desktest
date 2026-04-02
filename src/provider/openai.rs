use super::http_base::HttpProvider;

/// OpenAI provider — thin wrapper around [`HttpProvider`] with a default base URL.
pub struct OpenAiProvider;

impl OpenAiProvider {
    pub fn create(api_key: &str, model: &str) -> HttpProvider {
        HttpProvider::new(api_key, model, "https://api.openai.com", "OpenAI")
    }

    pub fn create_with_client(api_key: &str, model: &str, http: reqwest::Client) -> HttpProvider {
        HttpProvider::with_client(api_key, model, "https://api.openai.com", "OpenAI", http)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{LlmProvider, user_message};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_text_response(content: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": content
                }
            }]
        })
    }

    #[tokio::test]
    async fn test_openai_provider_defaults() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_text_response("Hello!")))
            .mount(&server)
            .await;

        let client = OpenAiProvider::create("sk-test", "gpt-4.1").with_base_url(&server.uri());
        let messages = vec![user_message("Hi")];
        let result = client.chat_completion(&messages, &[]).await.unwrap();

        assert_eq!(result.role, "assistant");
        assert_eq!(
            result.content.unwrap(),
            serde_json::Value::String("Hello!".into())
        );
    }
}
