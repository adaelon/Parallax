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
        "eam_initial_identity_v1" => initial_identity_example(input)?,
        "eam_person_fact_proposals_v1" => r#"{"fact_proposals":[]}"#.to_owned(),
        "eam_runtime_response_v1" => {
            r#"{"text":"Example response","citations":[],"operations":[]}"#.to_owned()
        }
        _ => "{}".to_owned(),
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

fn initial_identity_example(input: &str) -> Result<String, RuntimeError> {
    let input: Value = serde_json::from_str(input)
        .map_err(|error| RuntimeError::invalid_response(error.to_string()))?;
    let introduction = input
        .get("introduction")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            RuntimeError::invalid_response("initial identity input has no introduction array")
        })?;
    let evidence_refs = introduction
        .iter()
        .map(|item| {
            item.get("evidence_id")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    RuntimeError::invalid_response(
                        "initial identity introduction has no evidence ID",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::to_string(&json!({
        "profile": {
            "name": "Example",
            "expression_traits": "direct",
            "viewpoints": "evidence-aware",
            "value_priorities": "accuracy",
            "relationship_posture": "distinct counterpart",
            "own_goals": "support reflection"
        },
        "change_reason": "formed from the introduction",
        "evidence_refs": evidence_refs,
        "authored_by": "counterpart",
        "reflective_purpose": "preserved",
        "person_representation": "distinct_counterpart"
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
