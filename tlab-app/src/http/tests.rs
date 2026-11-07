use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use tlab::http::trace::get_current_traceparent;
use tower::ServiceExt;
use tracing::Instrument;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::layer::SubscriberExt;

use super::trace_requests;

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl io::Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn request_response_and_failure_logs_share_request_context_at_info() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .without_time()
                .with_writer(move || writer.clone()),
        );
    async {
        let router = trace_requests(
            Router::new()
                .route(
                    "/ok",
                    get(|| async {
                        let context = get_current_traceparent().unwrap();
                        async {
                            tokio::task::yield_now().await;
                            assert_eq!(get_current_traceparent().unwrap(), context);
                            tracing::info!("handler-event");
                        }
                        .instrument(tracing::info_span!("handler_child"))
                        .await;
                        StatusCode::OK
                    }),
                )
                .route(
                    "/error",
                    get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
                ),
        );
        let trace_id = "0af7651916cd43dd8448eb211c80319c";
        let parent = "b7ad6b7169203331";
        let valid = format!("00-{trace_id}-{parent}-01");
        let unsupported = format!("01-{trace_id}-{parent}-01");
        for (path, status, header) in [
            ("/ok", 200, Some(valid.as_str())),
            ("/missing", 404, Some(valid.as_str())),
            ("/error", 500, Some(valid.as_str())),
            ("/ok", 200, None),
            ("/ok", 200, Some("invalid")),
            ("/ok", 200, Some(unsupported.as_str())),
        ] {
            let start = logs.0.lock().unwrap().len();
            let mut request = Request::builder().uri(path);
            if let Some(header) = header {
                request = request.header("traceparent", header);
            }
            let request = request.body(Body::empty()).unwrap();
            assert_eq!(
                router
                    .clone()
                    .oneshot(request)
                    .await
                    .unwrap()
                    .status()
                    .as_u16(),
                status
            );
            let output = String::from_utf8(logs.0.lock().unwrap()[start..].to_vec()).unwrap();
            assert!(output.contains("started processing request"), "{output}");
            assert!(output.contains("finished processing request"), "{output}");
            assert!(output.contains(&format!("status={status}")), "{output}");
            assert!(output.contains("latency="), "{output}");
            assert!(output.contains("method=GET"), "{output}");
            assert!(output.contains(&format!("uri={path}")), "{output}");
            let continues_trace = header == Some(valid.as_str());
            let expected_parent = if continues_trace {
                parent
            } else {
                "0000000000000000"
            };
            assert!(
                output.contains(&format!("parent_span_id={expected_parent}")),
                "{output}"
            );
            let mut ids = Vec::new();
            for line in output.lines() {
                assert_eq!(
                    line.contains(&format!("trace_id={trace_id}")),
                    continues_trace,
                    "{line}"
                );
                let actual_trace = line
                    .split("trace_id=")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap();
                assert_eq!(actual_trace.len(), 32);
                assert_ne!(actual_trace, "00000000000000000000000000000000");
                let id = line
                    .split(" span_id=")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap();
                ids.push(id.trim_end_matches('}'));
            }
            assert!(ids.windows(2).all(|pair| pair[0] == pair[1]), "{output}");
            assert_eq!(output.contains("ERROR"), status == 500, "{output}");
            if status == 200 {
                assert!(output.contains("handler-event"), "{output}");
            }
        }
    }
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn concurrent_requests_keep_their_own_context_after_yielding() {
    let subscriber = tracing_subscriber::registry();
    async {
        let router = trace_requests(Router::new().route(
            "/",
            get(|| async {
                let before = get_current_traceparent().unwrap();
                tokio::task::yield_now().await;
                assert_eq!(get_current_traceparent().unwrap(), before);
                before.trace_id
            }),
        ));
        let request = |trace: &str| {
            Request::builder()
                .uri("/")
                .header("traceparent", format!("00-{trace}-b7ad6b7169203331-00"))
                .body(Body::empty())
                .unwrap()
        };
        let first = "11111111111111111111111111111111";
        let second = "22222222222222222222222222222222";
        let (a, b) = tokio::join!(
            router.clone().oneshot(request(first)),
            router.oneshot(request(second))
        );
        for (response, expected) in [(a, first), (b, second)] {
            let body = axum::body::to_bytes(response.unwrap().into_body(), 128)
                .await
                .unwrap();
            assert_eq!(body.as_ref(), expected.as_bytes());
        }
        assert!(get_current_traceparent().is_none());
    }
    .with_subscriber(subscriber)
    .await;
}
