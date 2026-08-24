use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use base64::Engine as _;
use serde::Deserialize;

use crate::compiler::{CompileOutcome, CompilerPool};
use crate::config::Config;
use crate::templates;

// Frontend bundle is embedded so the service ships as a single binary.
static EDITOR_JS: &str = include_str!("../static/editor.js");
static EDITOR_CSS: &str = include_str!("../static/editor.css");
static LIGHT_ICON: &str = include_str!("../static/icons/light.svg");
static ASLEEP_ICON: &str = include_str!("../static/icons/asleep.svg");

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: Arc<CompilerPool>,
}

pub fn router(state: AppState) -> Router {
    let max_source = state.config.max_source;
    let auth = state.config.auth.clone();
    let app = Router::new()
        .route("/", get(index))
        .route("/{slug}", get(editor))
        .route("/{slug}/compile", post(compile))
        .route("/static/editor.js", get(editor_js))
        .route("/static/editor.css", get(editor_css))
        .route("/static/icons/light.svg", get(light_icon))
        .route("/static/icons/asleep.svg", get(asleep_icon))
        .layer(DefaultBodyLimit::max(max_source))
        .with_state(state);
    let app = match auth {
        Some(auth) => app.layer(middleware::from_fn(move |req, next| {
            let auth = auth.clone();
            basic_auth(auth, req, next)
        })),
        None => app,
    };
    // healthz stays outside the auth layer
    app.route("/healthz", get(|| async { "ok" }))
}

async fn basic_auth(
    auth: crate::config::BasicAuth,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let expected =
        base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", auth.user, auth.pass));
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .is_some_and(|got| got == expected);
    if ok {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"typst-letter\"")],
            "authentication required",
        )
            .into_response()
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// JSON string safe for embedding in a <script> block.
fn json_for_script<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .expect("serializable")
        .replace('<', "\\u003c")
}

async fn index(State(state): State<AppState>) -> Html<String> {
    let items: String = templates::list_templates(&state.config.templates_dir)
        .iter()
        .map(|t| {
            format!(
                "<li><a href=\"/{slug}\"><span class=\"slug\">{slug}</span> — {title}</a></li>\n",
                slug = t.slug,
                title = html_escape(&t.title),
            )
        })
        .collect();
    let title = html_escape(&state.config.base_title);
    let year = chrono::Local::now().format("%Y");
    Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="/static/editor.css">
<meta name="theme-color" id="tc">
<script>(function(){{var m=matchMedia('(prefers-color-scheme:dark)'),t=localStorage.theme,h=document.documentElement,c=document.getElementById('tc');t=t=='dk'||t=='lt'?t:m.matches?'dk':'lt';localStorage.theme=t;h.className=t;function u(){{c.content=getComputedStyle(h).getPropertyValue('--tc')}}u();window.zb=function(){{t=t==='dk'?'lt':'dk';localStorage.theme=t;h.className=t;u()}}}})()</script>
</head>
<body class="index">
<header class="index-header">
<div class="brand">
<pre class="brand-logo" aria-hidden="true"><span>█   █▀▀ ▀█▀</span><span>█   █▀▀  █ </span><span>▀▀▀ ▀▀▀  ▀ </span></pre>
<span><strong>{title}</strong><span class="desc"> · private Typst letter editor</span></span>
</div>
<button class="theme" type="button" onclick="zb()" aria-label="Toggle theme" title="Toggle theme"><img src="/static/icons/light.svg" alt="Light mode"><img src="/static/icons/asleep.svg" alt="Dark mode"></button>
</header>
<main>
<h1>choose a template</h1>
<p class="intro">Edit beside a live PDF preview. Drafts stay in this browser tab.</p>
<ul class="templates">
{items}</ul>
</main>
<footer>
<span>© {year} Roman Ataman</span>
<span><a href="https://letters.atamanroman.dev">letters.atamanroman.dev</a> · <a href="https://polyformproject.org/licenses/noncommercial/1.0.0/">PolyForm Noncommercial 1.0.0</a></span>
</footer>
</body>
</html>
"#
    ))
}

async fn editor(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Html<String>, StatusCode> {
    if !templates::valid_slug(&slug) {
        return Err(StatusCode::NOT_FOUND);
    }
    let source = templates::read_template(&state.config.templates_dir, &slug)
        .ok_or(StatusCode::NOT_FOUND)?;
    let boot = json_for_script(&serde_json::json!({
        "slug": slug,
        "source": source,
        "debounceMs": state.config.debounce_ms,
    }));
    let title = html_escape(&format!("{} — {}", slug, state.config.base_title));
    Ok(Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="/static/editor.css">
<meta name="theme-color" id="tc">
<script>(function(){{var m=matchMedia('(prefers-color-scheme:dark)'),t=localStorage.theme,h=document.documentElement,c=document.getElementById('tc');t=t=='dk'||t=='lt'?t:m.matches?'dk':'lt';localStorage.theme=t;h.className=t;function u(){{c.content=getComputedStyle(h).getPropertyValue('--tc')}}u();window.zb=function(){{t=t==='dk'?'lt':'dk';localStorage.theme=t;h.className=t;u()}}}})()</script>
</head>
<body class="editor-page">
<script type="application/json" id="boot">{boot}</script>
<div id="app"></div>
<script src="/static/editor.js"></script>
</body>
</html>
"#
    )))
}

#[derive(Deserialize)]
struct CompileQuery {
    download: Option<String>,
}

async fn compile(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<CompileQuery>,
    body: Bytes,
) -> Response {
    if !templates::valid_slug(&slug) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if body.len() > state.config.max_source {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let Ok(source) = String::from_utf8(body.to_vec()) else {
        return (StatusCode::BAD_REQUEST, "body must be UTF-8").into_response();
    };
    match state.pool.compile(slug.clone(), source).await {
        CompileOutcome::Ok { pdf, warnings } => {
            let disposition = if query.download.as_deref() == Some("1") {
                "attachment"
            } else {
                "inline"
            };
            let date = chrono::Local::now().format("%Y-%m-%d");
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/pdf"),
            );
            headers.insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("{disposition}; filename=\"{slug}-{date}.pdf\""))
                    .expect("valid header"),
            );
            if !warnings.is_empty() {
                // ASCII-safe: serde_json escapes control chars, and we strip
                // non-ASCII via escaping in json_for_script-like manner.
                if let Ok(v) = HeaderValue::from_str(
                    &serde_json::to_string(&warnings)
                        .unwrap_or_default()
                        .replace(|c: char| !c.is_ascii() || c == '\n' || c == '\r', "?"),
                ) {
                    headers.insert("x-typst-warnings", v);
                }
            }
            (headers, pdf).into_response()
        }
        CompileOutcome::Failed { diags } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&diags).unwrap_or_else(|_| "[]".into()),
        )
            .into_response(),
        CompileOutcome::Busy => (
            StatusCode::TOO_MANY_REQUESTS,
            "compiler busy, retry shortly",
        )
            .into_response(),
        CompileOutcome::TimedOut | CompileOutcome::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "compilation failed internally",
        )
            .into_response(),
    }
}

async fn editor_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        EDITOR_JS,
    )
}

async fn editor_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        EDITOR_CSS,
    )
}

async fn light_icon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], LIGHT_ICON)
}

async fn asleep_icon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], ASLEEP_ICON)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn fixture() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("shared")).unwrap();
        std::fs::write(dir.path().join("business.typ"), "// Business letter\n= Hi").unwrap();
        std::fs::write(dir.path().join("personal.typ"), "// Personal\n= Yo").unwrap();
        let mut config = Config::from_toml("").unwrap();
        config.templates_dir = dir.path().to_path_buf();
        config.max_compiles_in_flight = 1;
        let pool = CompilerPool::new(&config).unwrap();
        (
            dir,
            AppState {
                config: Arc::new(config),
                pool,
            },
        )
    }

    fn req(method: &str, uri: &str, body: &str) -> axum::extract::Request {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn healthz_ok() {
        let (_d, state) = fixture();
        let res = router(state)
            .oneshot(req("GET", "/healthz", ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn index_lists_templates() {
        let (_d, state) = fixture();
        let res = router(state).oneshot(req("GET", "/", "")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec())
            .unwrap();
        assert!(body.contains("Business letter"));
        assert!(body.contains("/personal"));
        assert!(body.contains("© "));
        assert!(body.contains("Roman Ataman"));
        assert!(body.contains("polyformproject.org/licenses/noncommercial/1.0.0/"));
        assert!(body.contains("/static/icons/light.svg"));
        assert!(body.contains("/static/icons/asleep.svg"));
    }

    #[tokio::test]
    async fn theme_icons_are_embedded() {
        let (_d, state) = fixture();
        let app = router(state);
        for uri in ["/static/icons/light.svg", "/static/icons/asleep.svg"] {
            let res = app.clone().oneshot(req("GET", uri, "")).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(res.headers()[header::CONTENT_TYPE], "image/svg+xml");
            let body = res.into_body().collect().await.unwrap().to_bytes();
            assert!(body.starts_with(b"<svg"));
        }
    }

    #[tokio::test]
    async fn editor_embeds_source() {
        let (_d, state) = fixture();
        let res = router(state)
            .oneshot(req("GET", "/business", ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec())
            .unwrap();
        assert!(body.contains("Business letter"));
        assert!(body.contains("debounceMs"));
    }

    #[tokio::test]
    async fn bad_slugs_rejected() {
        let (_d, state) = fixture();
        let app = router(state);
        for uri in ["/shared", "/Nope", "/no_pe", "/missing"] {
            let res = app.clone().oneshot(req("GET", uri, "")).await.unwrap();
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{uri}");
        }
        let res = app
            .oneshot(req("POST", "/shared/compile", "= x"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn compile_returns_pdf_inline_and_attachment() {
        let (_d, state) = fixture();
        let app = router(state);
        let res = app
            .clone()
            .oneshot(req("POST", "/business/compile", "= Hello"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "application/pdf");
        let dispo = res.headers()[header::CONTENT_DISPOSITION].to_str().unwrap();
        assert!(dispo.starts_with("inline; filename=\"business-"), "{dispo}");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..4], b"%PDF");

        let res = app
            .oneshot(req("POST", "/business/compile?download=1", "= Hello"))
            .await
            .unwrap();
        let dispo = res.headers()[header::CONTENT_DISPOSITION].to_str().unwrap();
        assert!(dispo.starts_with("attachment;"), "{dispo}");
    }

    #[tokio::test]
    async fn compile_errors_are_json_diags() {
        let (_d, state) = fixture();
        let res = router(state)
            .oneshot(req("POST", "/business/compile", "#undefinedvar"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let diags: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(diags[0]["severity"], "error");
        assert_eq!(diags[0]["line"], 1);
    }

    #[tokio::test]
    async fn oversized_body_is_413() {
        let (_d, state) = fixture();
        let big = "x".repeat(state.config.max_source + 1);
        let res = router(state)
            .oneshot(req("POST", "/business/compile", &big))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn basic_auth_gates_everything_but_healthz() {
        let (_d, mut state) = fixture();
        let mut config = (*state.config).clone();
        config.auth = Some(crate::config::BasicAuth {
            user: "alice".into(),
            pass: "secret".into(),
        });
        state.config = Arc::new(config);
        let app = router(state);

        let res = app
            .clone()
            .oneshot(req("GET", "/healthz", ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app.clone().oneshot(req("GET", "/", "")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let cred = base64::engine::general_purpose::STANDARD.encode("alice:secret");
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, format!("Basic {cred}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
