use axum::Router;
use rand::RngCore;

const HTML: &str = include_str!("../assets/operator-dashboard.html");
const CSS: &str = include_str!("../assets/operator-dashboard.css");
const JAVASCRIPT: &str = include_str!("../assets/operator-dashboard.js");

pub struct ServeOptions {
    pub open_browser: bool,
}

pub fn serve(options: ServeOptions) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve_async(options))
}

async fn serve_async(options: ServeOptions) -> anyhow::Result<()> {
    let mut token_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut token_bytes);
    let token = hex::encode(token_bytes);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let url = format!("http://{address}/?token={token}");
    println!("Wire operator dashboard: {url}");
    use std::io::Write as _;
    std::io::stdout().flush()?;
    if options.open_browser
        && let Err(error) = open_browser(&url)
    {
        eprintln!("wire dash: could not open browser: {error}");
    }
    axum::serve(listener, router(token)).await?;
    Ok(())
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let child = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let child = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(windows)]
    let child = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    child.map(|_| ())
}
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    token: String,
    scan_lock: Arc<tokio::sync::Mutex<()>>,
}

fn router(token: String) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .route("/dashboard.css", get(stylesheet))
        .route("/dashboard.js", get(javascript))
        .route("/api/sessions", get(get_sessions))
        .route("/api/topology", get(get_topology))
        .route("/api/links", post(post_links))
        .route("/api/groups", post(post_groups))
        .with_state(AppState {
            token,
            scan_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
        .layer(middleware::map_response(security_headers))
}

async fn index() -> Html<&'static str> {
    Html(HTML)
}

async fn stylesheet() -> impl IntoResponse {
    ([(CONTENT_TYPE, "text/css; charset=utf-8")], CSS)
}

async fn javascript() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/javascript; charset=utf-8")],
        JAVASCRIPT,
    )
}

async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; connect-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    );
    response
}

fn authorized(headers: &HeaderMap, state: &AppState) -> bool {
    let token_matches = headers
        .get("X-Wire-Token")
        .and_then(|value| value.to_str().ok())
        == Some(state.token.as_str());
    token_matches && local_browser_request(headers)
}

fn local_browser_request(headers: &HeaderMap) -> bool {
    let Some(authority) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Ok(host_url) = reqwest::Url::parse(&format!("http://{authority}")) else {
        return false;
    };
    if !matches!(host_url.host_str(), Some("127.0.0.1" | "localhost")) {
        return false;
    }
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| origin.trim_end_matches('/') == format!("http://{authority}"))
}

fn error_response(status: StatusCode, message: &str, changed_sessions: Vec<String>) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": message,
            "changed_sessions": changed_sessions,
        })),
    )
        .into_response()
}

fn operator_error(error: crate::operator::OperatorError) -> Response {
    match error {
        crate::operator::OperatorError::Validation(message) => {
            error_response(StatusCode::BAD_REQUEST, &message, Vec::new())
        }
        crate::operator::OperatorError::Conflict(message) => {
            error_response(StatusCode::CONFLICT, &message, Vec::new())
        }
        crate::operator::OperatorError::Partial {
            message,
            changed_sessions,
        } => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &message,
            changed_sessions,
        ),
        crate::operator::OperatorError::Internal(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "operator action failed",
            Vec::new(),
        ),
    }
}

async fn get_sessions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return error_response(StatusCode::FORBIDDEN, "invalid launch token", Vec::new());
    }
    let _scan = state.scan_lock.lock().await;
    match tokio::task::spawn_blocking(crate::operator::collect_live_sessions).await {
        Ok(Ok(report)) => Json(report).into_response(),
        _ => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session inventory failed",
            Vec::new(),
        ),
    }
}

async fn get_topology(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return error_response(StatusCode::FORBIDDEN, "invalid launch token", Vec::new());
    }
    let _scan = state.scan_lock.lock().await;
    match tokio::task::spawn_blocking(crate::operator_topology::collect_topology).await {
        Ok(Ok(report)) => Json(report).into_response(),
        _ => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "topology inventory failed",
            Vec::new(),
        ),
    }
}

async fn post_links(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<crate::operator::LinkRequest>, JsonRejection>,
) -> Response {
    if !authorized(&headers, &state) {
        return error_response(StatusCode::FORBIDDEN, "invalid launch token", Vec::new());
    }
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            return error_response(error.status(), "request must be JSON", Vec::new());
        }
    };
    match tokio::task::spawn_blocking(move || crate::operator::link_local_sessions(request)).await {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err(error)) => operator_error(error),
        Err(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "operator action failed",
            Vec::new(),
        ),
    }
}

async fn post_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<crate::operator::GroupRequest>, JsonRejection>,
) -> Response {
    if !authorized(&headers, &state) {
        return error_response(StatusCode::FORBIDDEN, "invalid launch token", Vec::new());
    }
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            return error_response(error.status(), "request must be JSON", Vec::new());
        }
    };
    match tokio::task::spawn_blocking(move || crate::operator::create_local_group(request)).await {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err(error)) => operator_error(error),
        Err(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "operator action failed",
            Vec::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mutation_routes_require_token_and_json() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router("test-token".to_string()))
                .await
                .unwrap();
        });
        let client = reqwest::Client::new();
        let links = format!("http://{address}/api/links");

        let missing = client
            .post(&links)
            .json(&serde_json::json!({"sessions":["alice","bob"]}))
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::FORBIDDEN);

        let wrong = client
            .post(&links)
            .header("X-Wire-Token", "wrong")
            .json(&serde_json::json!({"sessions":["alice","bob"]}))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::FORBIDDEN);

        let text = client
            .post(&links)
            .header("X-Wire-Token", "test-token")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(text.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let sessions = client
            .get(format!("http://{address}/api/sessions"))
            .send()
            .await
            .unwrap();
        assert_eq!(sessions.status(), StatusCode::FORBIDDEN);

        let topology = format!("http://{address}/api/topology");
        let missing = client.get(&topology).send().await.unwrap();
        assert_eq!(missing.status(), StatusCode::FORBIDDEN);

        let wrong = client
            .get(&topology)
            .header("X-Wire-Token", "wrong")
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::FORBIDDEN);

        let rebound = client
            .get(&topology)
            .header("Host", "attacker.example")
            .header("X-Wire-Token", "test-token")
            .send()
            .await
            .unwrap();
        assert_eq!(rebound.status(), StatusCode::FORBIDDEN);

        let cross_origin = client
            .get(&topology)
            .header("Origin", "https://attacker.example")
            .header("X-Wire-Token", "test-token")
            .send()
            .await
            .unwrap();
        assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);

        let sessions = client
            .get(format!("http://{address}/api/sessions"))
            .header("X-Wire-Token", "test-token")
            .send()
            .await
            .unwrap();
        assert_eq!(sessions.status(), StatusCode::OK);

        let rebound = client
            .post(&links)
            .header("Host", "attacker.example")
            .header("X-Wire-Token", "test-token")
            .json(&serde_json::json!({"sessions":["alice","bob"]}))
            .send()
            .await
            .unwrap();
        assert_eq!(rebound.status(), StatusCode::FORBIDDEN);

        let cross_origin = client
            .post(&links)
            .header("Origin", "https://attacker.example")
            .header("X-Wire-Token", "test-token")
            .json(&serde_json::json!({"sessions":["alice","bob"]}))
            .send()
            .await
            .unwrap();
        assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);
        server.abort();
    }

    #[tokio::test]
    async fn dashboard_assets_are_served_with_local_security_contract() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router("test-token".to_string()))
                .await
                .unwrap();
        });
        let client = reqwest::Client::new();
        let page = client
            .get(format!("http://{address}/?token=test-token"))
            .send()
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(
            page.headers()
                .get("X-Frame-Options")
                .and_then(|value| value.to_str().ok()),
            Some("DENY")
        );
        let html = page.text().await.unwrap();
        assert!(html.contains("Wire operator"));
        assert!(html.contains("Link selected"));
        assert!(html.contains("Create group"));
        assert!(html.contains("aria-labelledby=\"group-title\""));
        for heading in ["Harness", "Project", "Machine", "Identity"] {
            assert!(
                html.contains(heading),
                "missing dashboard heading {heading}"
            );
        }

        let script = client
            .get(format!("http://{address}/dashboard.js"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(!script.contains("http://"));
        assert!(!script.contains("https://"));
        assert!(!script.contains("innerHTML"));
        assert!(script.contains("sessionStorage"));
        assert!(script.contains("aria-expanded"));
        assert!(script.contains("details-button"));
        assert!(script.contains("detail-row"));
        assert!(script.contains("Unknown"));
        assert!(script.contains("PID ${known(session.pid)}"));
        server.abort();
    }
}
