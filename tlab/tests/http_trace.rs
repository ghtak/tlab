use axum::http::Request;
use tlab::http::trace::{TraceparentData, get_current_traceparent, new_request_span};
use tracing_subscriber::{layer::SubscriberExt, registry::LookupSpan};

const TRACE: &str = "0af7651916cd43dd8448eb211c80319c";
const PARENT: &str = "b7ad6b7169203331";

fn request(header: Option<&str>) -> Request<()> {
    let mut builder = Request::builder().uri("/health");
    if let Some(header) = header {
        builder = builder.header("traceparent", header);
    }
    builder.body(()).unwrap()
}

fn context(req: &Request<()>) -> TraceparentData {
    new_request_span(req).in_scope(|| get_current_traceparent().unwrap())
}

fn assert_id(value: &str, length: usize) {
    assert_eq!(value.len(), length);
    assert!(
        value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert!(value.bytes().any(|b| b != b'0'));
}

#[test]
fn continues_trace_and_finds_context_from_child_spans() {
    tracing::subscriber::with_default(tracing_subscriber::registry(), || {
        let req = request(Some(&format!("00-{TRACE}-{PARENT}-01")));
        let span = new_request_span(&req);
        assert!(get_current_traceparent().is_none());
        let data = span.in_scope(|| {
            let data = get_current_traceparent().unwrap();
            assert_eq!(data.trace_id, TRACE);
            assert_id(&data.span_id, 16);
            assert_ne!(data.span_id, PARENT);
            tracing::info_span!("child").in_scope(|| {
                assert_eq!(get_current_traceparent().unwrap(), data);
            });
            // A nested HTTP request is a new root, regardless of the current span.
            let nested = new_request_span(&request(None));
            nested.with_subscriber(|(id, subscriber)| {
                let registry = subscriber
                    .downcast_ref::<tracing_subscriber::Registry>()
                    .unwrap();
                assert!(registry.span(id).unwrap().parent().is_none());
            });
            assert_ne!(
                nested
                    .in_scope(|| get_current_traceparent().unwrap())
                    .trace_id,
                TRACE
            );
            data
        });
        assert_ne!(context(&req).span_id, data.span_id);
        assert!(get_current_traceparent().is_none());
    });
}

#[test]
fn malformed_or_missing_headers_start_new_traces_without_panicking() {
    tracing::subscriber::with_default(tracing_subscriber::registry(), || {
        let valid = format!("00-{TRACE}-{PARENT}-01");
        let invalid = [
            String::new(),
            valid[..54].to_owned(),
            valid.replace(TRACE, &TRACE.to_uppercase()),
            valid.replace(TRACE, &"0".repeat(32)),
            valid.replace(PARENT, &"0".repeat(16)),
            valid.replacen("00-", "ff-", 1),
            format!("{valid}-extra"),
            format!("00-{TRACE}-{PARENT}-gg"),
            format!("00-{TRACE}-{PARENT}-0é"),
            format!("01-{TRACE}-{PARENT}-01extra"),
            format!("01-{TRACE}-{PARENT}-01"),
            format!("01-{TRACE}-{PARENT}-01-extra"),
        ];
        for header in std::iter::once(None).chain(invalid.iter().map(|s| Some(s.as_str()))) {
            let data = context(&request(header));
            assert_id(&data.trace_id, 32);
            assert_id(&data.span_id, 16);
            assert_ne!(data.trace_id, TRACE, "{header:?}");
        }
        let mut repeated = request(Some(&valid));
        repeated
            .headers_mut()
            .append("traceparent", valid.parse().unwrap());
        assert_ne!(context(&repeated).trace_id, TRACE);
    });
}

#[test]
fn accepts_valid_flags_without_sampling_logs() {
    tracing::subscriber::with_default(tracing_subscriber::registry(), || {
        for header in [
            format!("00-{TRACE}-{PARENT}-00"),
            format!("00-{TRACE}-{PARENT}-fe"),
        ] {
            assert_eq!(context(&request(Some(&header))).trace_id, TRACE);
        }
    });
}

#[test]
fn missing_unsupported_or_filtered_subscribers_return_none() {
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
        assert!(get_current_traceparent().is_none());
        new_request_span(&request(None)).in_scope(|| assert!(get_current_traceparent().is_none()));
    });
    let filtered =
        tracing_subscriber::registry().with(tracing_subscriber::filter::LevelFilter::ERROR);
    tracing::subscriber::with_default(filtered, || {
        assert!(new_request_span(&request(None)).is_disabled());
        tracing::error_span!("unrelated").in_scope(|| assert!(get_current_traceparent().is_none()));
    });
    // A subscriber without a Registry cannot store request extensions.
    struct Subscriber;
    impl tracing::Subscriber for Subscriber {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {}
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    tracing::subscriber::with_default(Subscriber, || {
        new_request_span(&request(None)).in_scope(|| assert!(get_current_traceparent().is_none()));
    });
}
