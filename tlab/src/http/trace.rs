//! Request spans linked to incoming W3C trace context.

use axum::http::Request;
use tracing::{Span, info_span};
use tracing_subscriber::{Registry, registry::LookupSpan};
use uuid::Uuid;

const ROOT_SPAN_ID: &str = "0000000000000000";
const TRACEPARENT_VERSION: &str = "00";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceparentData {
    pub trace_id: String,
    pub span_id: String,
}

/// Creates an INFO request span, continuing a valid incoming trace ID.
/// Missing, malformed, repeated, or unsupported-version headers start a new trace.
/// Only traceparent version 00 is supported.
/// Sampling flags do not control local log filtering.
pub fn new_request_span<B>(req: &Request<B>) -> Span {
    let mut headers = req.headers().get_all("traceparent").iter();
    let context = headers
        .next()
        .filter(|_| headers.next().is_none())
        .and_then(|value| value.to_str().ok())
        .and_then(parse_traceparent);
    let (trace_id, parent_span_id) =
        context.unwrap_or_else(|| (Uuid::new_v4().simple().to_string(), ROOT_SPAN_ID.into()));
    let span_id = new_span_id();
    let span = info_span!(
        parent: None,
        "http_request",
        method = %req.method(),
        uri = %req.uri(),
        trace_id = %trace_id,
        span_id = %span_id,
        parent_span_id = %parent_span_id,
    );
    span.with_subscriber(|(id, subscriber)| {
        if let Some(registry) = subscriber.downcast_ref::<Registry>()
            && let Some(span_ref) = registry.span(id)
        {
            span_ref
                .extensions_mut()
                .insert(TraceparentData { trace_id, span_id });
        }
    });
    span
}

/// Returns the nearest request context in the current span's ancestry.
/// Requires an enabled request span backed by a tracing-subscriber Registry.
/// Spawned tasks must carry the request span explicitly, e.g. with `Instrument`.
pub fn get_current_traceparent() -> Option<TraceparentData> {
    Span::current()
        .with_subscriber(|(id, subscriber)| {
            let registry = subscriber.downcast_ref::<Registry>()?;
            registry
                .span(id)?
                .scope()
                .find_map(|span| span.extensions().get::<TraceparentData>().cloned())
        })
        .flatten()
}

fn new_span_id() -> String {
    loop {
        let id = Uuid::new_v4().simple().to_string();
        let id = &id[..16];
        if id != ROOT_SPAN_ID {
            return id.to_owned();
        }
    }
}

fn parse_traceparent(value: &str) -> Option<(String, String)> {
    let mut parts = value.split('-');
    let (version, trace_id, parent_span_id, flags) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);

    (parts.next().is_none()
        && version == TRACEPARENT_VERSION
        && is_valid_id(trace_id, 32)
        && is_valid_id(parent_span_id, 16)
        && is_hex(flags, 2))
    .then(|| (trace_id.to_owned(), parent_span_id.to_owned()))
}

fn is_valid_id(value: &str, length: usize) -> bool {
    is_hex(value, length) && value.bytes().any(|b| b != b'0')
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
