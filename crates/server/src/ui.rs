//! Minimal Web UI served by lattice-server.

use axum::{
    http::{header, HeaderValue},
    response::{Html, IntoResponse},
};

const INDEX_HTML: &str = include_str!("ui/index.html");
const APP_CSS: &str = include_str!("ui/app.css");
const APP_JS: &str = include_str!("ui/app.js");

pub async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub async fn styles() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/css; charset=utf-8"),
        )],
        APP_CSS,
    )
}

pub async fn script() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/javascript; charset=utf-8"),
        )],
        APP_JS,
    )
}
