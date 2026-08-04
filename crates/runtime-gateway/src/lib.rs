//! S06 model runtime gateway.
//!
//! The gateway translates trusted Core values into one strict domain contract
//! and adapts it to supported provider protocols. It never receives a
//! repository, Vault key, or action tool.

mod adapter;
mod deepseek;
mod fallback;
mod transport;

pub use adapter::OpenAiResponsesRuntime;
pub use fallback::FallbackRuntime;
pub use transport::{
    HttpResponsesTransport, InvocationKind, OutboundContextSource, OutboundDisclosureRecord,
    ResponsesTransport, RuntimeProtocol, RuntimeTarget, RuntimeTargetError, RuntimeTargetKind,
    TransportError, TransportErrorKind, validate_responses_bearer_token,
};
