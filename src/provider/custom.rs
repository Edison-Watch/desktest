use super::http_base::HttpProvider;

/// Custom provider for OpenAI-compatible endpoints — thin wrapper around [`HttpProvider`].
pub struct CustomProvider;

impl CustomProvider {
    pub fn create(api_key: &str, model: &str, base_url: &str) -> HttpProvider {
        HttpProvider::new(api_key, model, base_url, "Custom")
    }

    pub fn create_with_client(api_key: &str, model: &str, base_url: &str, http: reqwest::Client) -> HttpProvider {
        HttpProvider::with_client(api_key, model, base_url, "Custom", http)
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
    async fn test_custom_provider_with_base_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_text_response("Custom!")))
            .mount(&server)
            .await;

        let provider = CustomProvider::create("sk-test", "local-model", &server.uri());
        let messages = vec![user_message("Hi")];
        let result = provider.chat_completion(&messages, &[]).await.unwrap();

        assert_eq!(result.role, "assistant");
        assert_eq!(
            result.content.unwrap(),
            serde_json::Value::String("Custom!".into())
        );
    }
}
