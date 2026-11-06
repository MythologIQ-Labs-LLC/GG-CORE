//! Typed (non-generative) inference messages: embeddings and classification.
//!
//! Additive to protocol V1: older clients and daemons never send or expect
//! these frames. Every field is a closed type, so an unknown task, an
//! ambiguous input shape or an unknown result kind fails deserialization
//! instead of falling back to another path.

use serde::{Deserialize, Serialize};

use super::protocol_types::{InferenceErrorCode, RequestId};
use crate::engine::{InferenceCapability, InferenceInput, InferenceOutput};

/// Protocol feature advertised in `handshake_ack.features`.
pub const TYPED_INFERENCE_FEATURE: &str = "typed_inference_v1";

/// A typed task the daemon may serve. Closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypedTask {
    Embedding,
    TextClassification,
}

impl TypedTask {
    pub fn capability(self) -> InferenceCapability {
        match self {
            Self::Embedding => InferenceCapability::Embedding,
            Self::TextClassification => InferenceCapability::TextClassification,
        }
    }
}

/// Exactly one of a single text or a batch of texts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TypedInputWire", into = "TypedInputWire")]
pub enum TypedInput {
    Single(String),
    Batch(Vec<String>),
}

/// Strict wire form: `{"text": ".."}` or `{"texts": [..]}`, never both,
/// never neither, no other fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedInputWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    texts: Option<Vec<String>>,
}

impl TryFrom<TypedInputWire> for TypedInput {
    type Error = &'static str;
    fn try_from(wire: TypedInputWire) -> Result<Self, Self::Error> {
        match (wire.text, wire.texts) {
            (Some(text), None) => Ok(Self::Single(text)),
            (None, Some(texts)) => Ok(Self::Batch(texts)),
            _ => Err("typed input requires exactly one of `text` or `texts`"),
        }
    }
}

impl From<TypedInput> for TypedInputWire {
    fn from(input: TypedInput) -> Self {
        match input {
            TypedInput::Single(text) => Self {
                text: Some(text),
                texts: None,
            },
            TypedInput::Batch(texts) => Self {
                text: None,
                texts: Some(texts),
            },
        }
    }
}

impl TypedInput {
    pub fn into_engine(self) -> InferenceInput {
        match self {
            Self::Single(text) => InferenceInput::Text(text),
            Self::Batch(texts) => InferenceInput::TextBatch(texts),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedInferenceRequest {
    pub request_id: RequestId,
    pub model_id: String,
    pub task: TypedTask,
    pub input: TypedInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embedding {
    pub dimensions: usize,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelScore {
    pub label: String,
    pub confidence: f32,
}

/// A typed result. `kind` tags the shape explicitly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypedResult {
    Embedding(Embedding),
    EmbeddingBatch {
        items: Vec<Embedding>,
    },
    Classification {
        label: String,
        confidence: f32,
        all_labels: Vec<LabelScore>,
    },
}

impl TypedResult {
    /// Convert an engine output already validated by `run_typed_controlled`.
    /// Any other output kind is refused rather than coerced.
    pub fn from_engine(output: InferenceOutput) -> Result<Self, String> {
        let embed = |e: crate::engine::EmbeddingResult| Embedding {
            dimensions: e.dimensions,
            vector: e.vector,
        };
        match output {
            InferenceOutput::Embedding(e) => Ok(Self::Embedding(embed(e))),
            InferenceOutput::EmbeddingBatch(b) => Ok(Self::EmbeddingBatch {
                items: b.into_iter().map(embed).collect(),
            }),
            InferenceOutput::Classification(c) => Ok(Self::Classification {
                label: c.label,
                confidence: c.confidence,
                all_labels: c
                    .all_labels
                    .into_iter()
                    .map(|(label, confidence)| LabelScore { label, confidence })
                    .collect(),
            }),
            _ => Err("model output type does not match a typed task".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedInferenceResponse {
    pub request_id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<TypedResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<InferenceErrorCode>,
}

impl TypedInferenceResponse {
    pub fn success(request_id: RequestId, result: TypedResult) -> Self {
        Self {
            request_id,
            result: Some(result),
            error: None,
            error_code: None,
        }
    }

    pub fn error(request_id: RequestId, error: String, code: InferenceErrorCode) -> Self {
        Self {
            request_id,
            result: None,
            error: Some(error),
            error_code: Some(code),
        }
    }
}

/// Wire name of a model capability, for `models_response.capabilities`.
pub fn capability_name(capability: InferenceCapability) -> &'static str {
    match capability {
        InferenceCapability::TextGeneration => "text_generation",
        InferenceCapability::Embedding => "embedding",
        InferenceCapability::TextClassification => "text_classification",
        InferenceCapability::NamedEntityRecognition => "named_entity_recognition",
    }
}

#[cfg(test)]
#[path = "protocol_typed_tests.rs"]
mod tests;
