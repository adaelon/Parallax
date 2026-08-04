//! S06 model runtime gateway.
//!
//! The gateway translates trusted Core values into one strict Responses v2
//! contract. It never receives a repository, Vault key, or action tool.

mod adapter;
mod fallback;
mod transport;

pub use adapter::OpenAiResponsesRuntime;
pub use fallback::FallbackRuntime;
pub use transport::{
    HttpResponsesTransport, InvocationKind, OutboundContextSource, OutboundDisclosureRecord,
    ResponsesTransport, RuntimeTarget, RuntimeTargetError, RuntimeTargetKind, TransportError,
    TransportErrorKind,
};
