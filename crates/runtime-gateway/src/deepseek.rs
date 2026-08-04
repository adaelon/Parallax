use eam_core::RuntimeError;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn request_json(
    model: &str,
    instructions: &str,
    input: &str,
    schema_name: &str,
    schema: &Value,
) -> Result<String, RuntimeError> {
    let example = match schema_name {
        "eam_person_turn_classification_v1" => r#"{"classification":"question"}"#,
        "eam_runtime_response_v1" => {
            r#"{"text":"Example response","citations":[],"operations":[]}"#
        }
        _ => "{}",
    };
    let system_content = format!(
        "{instructions}\nReturn exactly one JSON object matching the JSON Schema below. Do not include Markdown fences or any text outside the JSON object.\nJSON Schema name: {schema_name}\nExample JSON object:\n{example}\nJSON Schema:\n{schema}"
    );
    serde_json::to_string(&json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": system_content
            },
            {
                "role": "user",
                "content": input
            }
        ],
        "response_format": {
            "type": "json_object"
        },
        "thinking": {
            "type": "disabled"
        },
        "stream": false
    }))
    .map_err(|error| RuntimeError::invalid_response(error.to_string()))
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Deserialize)]
struct ChatCompletionChoice {
    index: u64,
    finish_reason: String,
    message: ChatCompletionMessage,
}

#[derive(Deserialize)]
struct ChatCompletionMessage {
    content: Option<String>,
}

pub(crate) fn output_text(body: &str) -> Result<String, RuntimeError> {
    let response: ChatCompletionResponse = serde_json::from_str(body)
        .map_err(|error| RuntimeError::invalid_response(error.to_string()))?;
    let choice = response
        .choices
        .into_iter()
        .find(|choice| choice.index == 0)
        .ok_or_else(|| {
            RuntimeError::invalid_response("DeepSeek response has no choice at index 0")
        })?;
    if choice.finish_reason != "stop" {
        return Err(RuntimeError::invalid_response(
            "DeepSeek completion did not finish normally",
        ));
    }
    choice
        .message
        .content
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| RuntimeError::invalid_response("DeepSeek response has no message content"))
}
