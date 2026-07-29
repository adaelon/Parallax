//! S06 model runtime gateway.
//!
//! The gateway translates trusted Core values into one strict Responses v1
//! contract. It never receives a repository, Vault key, or action tool.

mod adapter;
mod fallback;
mod transport;

pub use adapter::OpenAiResponsesRuntime;
pub use fallback::FallbackRuntime;
pub use transport::{
    HttpResponsesTransport, InvocationKind, OPENAI_CLOUD_MODEL, OPENAI_LOCAL_MODEL,
    OutboundDisclosureRecord, ResponsesTransport, RuntimeTarget, RuntimeTargetKind, TransportError,
    TransportErrorKind,
};
