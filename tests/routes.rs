use axum::body::Body;
use axum::extract::Json as JsonExtract;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router as AxumRouter};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use srvcs_movingaverage::{api::Deps, health, router, telemetry};
use tower::ServiceExt;

const DEAD_URL: &str = "http://127.0.0.1:1";

async fn serve(app: AxumRouter) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Mock `srvcs-sum` that ACTUALLY COMPUTES the integer sum of the `values`
/// array and returns `{"values", "result": <i64>}`.
async fn spawn_computing_sum() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|JsonExtract(req): JsonExtract<Value>| async move {
            let sum: i64 = req["values"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_i64).sum())
                .unwrap_or(0);
            Json(json!({ "values": req["values"], "result": sum }))
        }),
    );
    serve(app).await
}

/// Mock `srvcs-floatdivide` that ACTUALLY COMPUTES `a / b` as an `f64`.
async fn spawn_computing_floatdivide() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|JsonExtract(req): JsonExtract<Value>| async move {
            let a = req["a"].as_f64().unwrap_or(0.0);
            let b = req["b"].as_f64().unwrap_or(1.0);
            Json(json!({ "a": a, "b": b, "result": a / b }))
        }),
    );
    serve(app).await
}

/// Mock that always answers with a fixed status + body (used to simulate a
/// `422` rejection forwarded from a dependency).
async fn spawn_fixed(status: StatusCode, body: Value) -> String {
    let app = AxumRouter::new().route(
        "/",
        post(move || {
            let body = body.clone();
            async move { (status, Json(body)) }
        }),
    );
    serve(app).await
}

fn app(sum_url: &str, floatdivide_url: &str) -> axum::Router {
    router(
        telemetry::metrics_handle_for_tests(),
        Deps {
            sum_url: sum_url.to_string(),
            floatdivide_url: floatdivide_url.to_string(),
        },
    )
}

async fn eval(
    sum_url: &str,
    floatdivide_url: &str,
    values: Value,
    window: i64,
) -> (StatusCode, Value) {
    let res = app(sum_url, floatdivide_url)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "values": values, "window": window }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn status_of(uri: &str) -> StatusCode {
    app(DEAD_URL, DEAD_URL)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

/// Assert that a JSON array of numbers matches `expected` element-wise within
/// 1e-9.
fn approx_list(got: &Value, expected: &[f64]) -> bool {
    let arr = match got.as_array() {
        Some(a) => a,
        None => return false,
    };
    if arr.len() != expected.len() {
        return false;
    }
    arr.iter()
        .zip(expected)
        .all(|(g, e)| g.as_f64().map(|x| (x - e).abs() < 1e-9) == Some(true))
}

// --- Standard endpoints ---

#[tokio::test]
async fn healthz_ok() {
    assert_eq!(status_of("/healthz").await, StatusCode::OK);
}

#[tokio::test]
async fn readyz_reflects_state() {
    health::set_ready(true);
    assert_eq!(status_of("/readyz").await, StatusCode::OK);
}

#[tokio::test]
async fn openapi_ok() {
    assert_eq!(status_of("/openapi.json").await, StatusCode::OK);
}

// --- Correctness cases, exercised against REAL computing dependencies ---

#[tokio::test]
async fn canonical_window_two() {
    let sum = spawn_computing_sum().await;
    let div = spawn_computing_floatdivide().await;
    let (status, body) = eval(&sum, &div, json!([1, 2, 3, 4]), 2).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        approx_list(&body["result"], &[1.5, 2.5, 3.5]),
        "got {:?}",
        body["result"]
    );
    assert_eq!(body["values"], json!([1, 2, 3, 4]));
    assert_eq!(body["window"], json!(2));
}

#[tokio::test]
async fn window_one_is_each_element() {
    let sum = spawn_computing_sum().await;
    let div = spawn_computing_floatdivide().await;
    let (status, body) = eval(&sum, &div, json!([2, 4, 6]), 1).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        approx_list(&body["result"], &[2.0, 4.0, 6.0]),
        "got {:?}",
        body["result"]
    );
}

#[tokio::test]
async fn window_equals_len_is_single_mean() {
    let sum = spawn_computing_sum().await;
    let div = spawn_computing_floatdivide().await;
    let (status, body) = eval(&sum, &div, json!([1, 2, 3, 4]), 4).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        approx_list(&body["result"], &[2.5]),
        "got {:?}",
        body["result"]
    );
}

#[tokio::test]
async fn window_three_over_five() {
    let sum = spawn_computing_sum().await;
    let div = spawn_computing_floatdivide().await;
    // [10,20,30,40,50] window 3 -> [20, 30, 40]
    let (status, body) = eval(&sum, &div, json!([10, 20, 30, 40, 50]), 3).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        approx_list(&body["result"], &[20.0, 30.0, 40.0]),
        "got {:?}",
        body["result"]
    );
}

#[tokio::test]
async fn handles_negatives() {
    let sum = spawn_computing_sum().await;
    let div = spawn_computing_floatdivide().await;
    // [-2, 0, 2, 4] window 2 -> [-1, 1, 3]
    let (status, body) = eval(&sum, &div, json!([-2, 0, 2, 4]), 2).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        approx_list(&body["result"], &[-1.0, 1.0, 3.0]),
        "got {:?}",
        body["result"]
    );
}

// --- Error / edge cases ---

#[tokio::test]
async fn window_zero_is_422_with_no_calls() {
    let (status, _) = eval(DEAD_URL, DEAD_URL, json!([1, 2, 3]), 0).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn window_too_large_is_422_with_no_calls() {
    let (status, _) = eval(DEAD_URL, DEAD_URL, json!([1, 2, 3]), 4).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn forwards_422_from_sum() {
    let sum = spawn_fixed(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "error": "value is not a number" }),
    )
    .await;
    let div = spawn_computing_floatdivide().await;
    let (status, body) = eval(&sum, &div, json!([1, "nope", 3]), 2).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "value is not a number");
}

#[tokio::test]
async fn degrades_when_sum_unreachable() {
    let div = spawn_computing_floatdivide().await;
    let (status, body) = eval(DEAD_URL, &div, json!([1, 2, 3]), 2).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-sum");
}

#[tokio::test]
async fn degrades_when_floatdivide_unreachable() {
    let sum = spawn_computing_sum().await;
    let (status, body) = eval(&sum, DEAD_URL, json!([1, 2, 3]), 2).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-floatdivide");
}
