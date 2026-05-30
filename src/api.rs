use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::{OpenApi, ToSchema};

use crate::client::{self, DepError};

pub const SERVICE: &str = "srvcs-movingaverage";
pub const CONCERN: &str = "statistics: moving (sliding-window) average";
pub const DEPENDS_ON: &[&str] = &["srvcs-sum", "srvcs-floatdivide"];

/// Dependency endpoints, injected as router state so tests can point them at
/// mock services.
#[derive(Clone)]
pub struct Deps {
    pub sum_url: String,
    pub floatdivide_url: String,
}

#[derive(Serialize, ToSchema)]
pub struct Info {
    pub service: &'static str,
    pub concern: &'static str,
    pub depends_on: Vec<&'static str>,
}

/// `GET /` — service identity (srvcs service standard).
#[utoipa::path(get, path = "/", responses((status = 200, body = Info)))]
pub async fn index() -> Json<Info> {
    Json(Info {
        service: SERVICE,
        concern: CONCERN,
        depends_on: DEPENDS_ON.to_vec(),
    })
}

#[derive(Deserialize, ToSchema)]
pub struct EvalRequest {
    /// The list of numbers to slide a window over.
    #[schema(value_type = Object)]
    pub values: Vec<Value>,
    /// The window size. Must be `>= 1` and `<= values.len()`.
    pub window: i64,
}

#[derive(Serialize, ToSchema)]
pub struct MovingAverageResponse {
    #[schema(value_type = Object)]
    pub values: Vec<Value>,
    pub window: i64,
    /// The list of windowed averages, as `f64`s.
    #[schema(value_type = Object)]
    pub result: Vec<f64>,
}

fn ok(values: Vec<Value>, window: i64, result: Vec<f64>) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "values": values, "window": window, "result": result })),
    )
        .into_response()
}

fn unprocessable(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": message })),
    )
        .into_response()
}

fn degraded(dependency: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "dependency unavailable", "dependency": dependency })),
    )
        .into_response()
}

/// Forward a dependency's response verbatim (used to propagate `422` for
/// invalid input from a leaf dependency).
fn forward(status: u16, body: Value) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (code, Json(body)).into_response()
}

/// A reachable dependency answered `200` but its body lacked a numeric
/// `result`. That is a contract violation we cannot recover from, so surface a
/// `500` rather than guessing.
fn malformed(dependency: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(
            json!({ "error": "dependency returned a malformed result", "dependency": dependency }),
        ),
    )
        .into_response()
}

/// Call one dependency at `url` with `body`, mapping its outcome to either the
/// parsed response body (on `200`) or an early-return `Response` the caller
/// should surface verbatim:
///
/// - unreachable / non-`200`/`422` -> `503` degraded
/// - `422` -> forwarded `422` (the dependency rejected the input)
async fn ask(url: &str, body: &Value, dependency: &str) -> Result<Value, Response> {
    match client::call(url, body).await {
        Err(DepError::Unreachable) => Err(degraded(dependency)),
        Ok((200, body)) => Ok(body),
        Ok((422, body)) => Err(forward(422, body)),
        Ok(_) => Err(degraded(dependency)),
    }
}

/// `POST /` — the moving (sliding-window) average of a list of numbers.
///
/// This service owns the *control flow* but delegates every arithmetic step to
/// its dependencies, exactly as specified. For each start `i` in
/// `0..=(values.len() - window)` it takes the window slice `values[i..i+window]`,
/// asks `srvcs-sum` for its sum, then asks `srvcs-floatdivide` to divide that
/// sum by `window`; the quotient is the windowed average.
///
/// `window` must be `>= 1` and `<= values.len()`, otherwise the request is
/// `422`. Validation of the element values themselves is propagated from
/// `srvcs-sum`'s `422`.
#[utoipa::path(
    post,
    path = "/",
    request_body = EvalRequest,
    responses(
        (status = 200, body = MovingAverageResponse),
        (status = 422, description = "window out of range, or a dependency rejected an input (forwarded)"),
        (status = 500, description = "a dependency returned a malformed result"),
        (status = 503, description = "a dependency is unavailable")
    )
)]
pub async fn evaluate(State(deps): State<Deps>, Json(req): Json<EvalRequest>) -> Response {
    let len = req.values.len() as i64;
    if req.window < 1 {
        return unprocessable("window must be >= 1");
    }
    if req.window > len {
        return unprocessable("window must be <= values.len()");
    }

    let window = req.window;
    let mut result: Vec<f64> = Vec::new();

    // For each start i in 0..=(len - window).
    let mut i: usize = 0;
    let last = (len - window) as usize;
    while i <= last {
        let w: Vec<Value> = req.values[i..i + window as usize].to_vec();

        // ws = sum(w).result
        let sum_body = match ask(&deps.sum_url, &json!({ "values": w }), "srvcs-sum").await {
            Ok(body) => body,
            Err(resp) => return resp,
        };
        let ws = match sum_body.get("result").and_then(Value::as_i64) {
            Some(s) => s,
            None => return malformed("srvcs-sum"),
        };

        // avg = floatdivide(ws, window).result
        let div_body = match ask(
            &deps.floatdivide_url,
            &json!({ "a": ws, "b": window }),
            "srvcs-floatdivide",
        )
        .await
        {
            Ok(body) => body,
            Err(resp) => return resp,
        };
        let avg = match div_body.get("result").and_then(Value::as_f64) {
            Some(a) => a,
            None => return malformed("srvcs-floatdivide"),
        };

        result.push(avg);
        i += 1;
    }

    ok(req.values, window, result)
}

#[derive(OpenApi)]
#[openapi(
    paths(index, evaluate),
    components(schemas(Info, EvalRequest, MovingAverageResponse))
)]
pub struct ApiDoc;

/// Serve OpenAPI document
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_documents_routes() {
        let doc = ApiDoc::openapi();
        let root = doc.paths.paths.get("/").expect("path / present");
        assert!(root.get.is_some());
        assert!(root.post.is_some());
    }

    #[tokio::test]
    async fn index_reports_all_dependencies() {
        let Json(info) = index().await;
        assert_eq!(info.service, "srvcs-movingaverage");
        assert_eq!(info.concern, "statistics: moving (sliding-window) average");
        assert_eq!(info.depends_on, vec!["srvcs-sum", "srvcs-floatdivide"]);
    }
}
