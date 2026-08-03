use axum::{Extension, Json};
use axum::http::StatusCode;
use axum::response::Redirect;
use axum::{
    extract::Form,
    response::Html,
    routing::{get, post},
    Router,
};
use tower_service::Service;
use worker::*;

use serde::{Deserialize, Serialize};
use worker::{
    console_error, Env, Fetch, Headers, Method, Request, RequestInit,
};

const SITEVERIFY_URL: &str =
    "https://challenges.cloudflare.com/turnstile/v0/siteverify";

#[derive(Debug, Serialize)]
struct TurnstileRequest<'a> {
    secret: &'a str,
    response: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnstileResult {
    pub success: bool,

    #[serde(default, rename = "error-codes")]
    pub error_codes: Vec<String>,

    #[serde(default)]
    pub challenge_ts: Option<String>,

    #[serde(default)]
    pub hostname: Option<String>,

    #[serde(default)]
    pub action: Option<String>,

    #[serde(default)]
    pub cdata: Option<String>,

    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

impl TurnstileResult {
    fn internal_error() -> Self {
        Self {
            success: false,
            error_codes: vec!["internal-error".to_string()],
            challenge_ts: None,
            hostname: None,
            action: None,
            cdata: None,
            metadata: None,
        }
    }
}

async fn validate_turnstile_inner(
    env: Env,
    token: &str,
) -> worker::Result<TurnstileResult> {
    // IMPLICIT DEPENDENCY: will need to change this to regular .env and env var fallback if we switch off CF Workers
    let secret = env.secret("TURNSTILE_SECRET_KEY").unwrap().to_string();
    let body = serde_json::to_string(&TurnstileRequest {
        secret: &secret,
        response: token,
    })
    .map_err(|error| worker::Error::RustError(error.to_string()))?;

    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(body.into()));

    let request = Request::new_with_init(SITEVERIFY_URL, &init)?;
    let mut response = Fetch::Request(request).send().await?;

    response.json::<TurnstileResult>().await
}

pub async fn validate_turnstile(
    env: Env,
    token: &str,
) -> TurnstileResult {
    match validate_turnstile_inner(env, token).await {
        Ok(result) => result,
        Err(error) => {
            console_error!("Turnstile validation error: {error}");
            TurnstileResult::internal_error()
        }
    }
}

// HIDDEN DEPENDENCY: we utilize cf worker's static file server
// I believe this is far from lock-in as it is trivial to add static file serving in another deployment fashion if needed

// EXPLICIT DEPENDENCY THAT I DON'T LIKE: we depend on cloudflare turnstile for user gating

async fn router(env: Env) -> Router {
    Router::new()
        .route("/api/contacts", post(accept_form))
        .route("/", get(Redirect::permanent("/index.html")))
        .layer(Extension(env))
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct Input {
    #[serde(rename(deserialize = "cf-turnstile-response"))]
    cf_turnstile_response: String,
    first_name: String,
    last_name: String,
    email: String,
    phone: String,
    howhear: String,
    comment: String,
}

#[worker::send]
async fn accept_form(Extension(env): Extension<Env>, Form(input): Form<Input>) -> Result<Redirect, (StatusCode, Json<TurnstileResult>)> {
    console_log!("{:#?}", &input);
    let validation =
        validate_turnstile( env, &input.cf_turnstile_response).await;

    if !validation.success {
        if !validation.success {
        return Err((
            StatusCode::FORBIDDEN,
            Json(validation),
        ));
    }
    }
    Ok(Redirect::to("/pages/thanks"))
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<axum::http::Response<axum::body::Body>> {
    Ok(router(env).await.call(req).await?)
}
