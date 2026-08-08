mod github;
use axum::http::StatusCode;
use axum::response::Redirect;
use axum::{
    body::Bytes,
    extract::Form,
    extract::Path,
    routing::{get, post},
    Router,
};
use axum::{Extension, Json};
use http::HeaderMap;
use tower_service::Service;
use worker::*;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use worker::{console_error, Env, Fetch, Headers, Method, Request, RequestInit};

const SITEVERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

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

async fn validate_turnstile_inner(env: &Env, token: &str) -> worker::Result<TurnstileResult> {
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

pub async fn validate_turnstile(env: &Env, token: &str) -> TurnstileResult {
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
        .route("/api/contacts/{group_location}", post(accept_form))
        .route(
            "/api/content/{group_location}/{content_type}/pr",
            post(open_content_pr),
        )
        .route("/", get(Redirect::permanent("/index.html")))
        .route("/api/discord", post(discord_interaction))
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
async fn accept_form(
    Extension(env): Extension<Env>,
    Path(group_location): Path<String>,
    Form(input): Form<Input>,
) -> Result<Redirect, (StatusCode, Json<TurnstileResult>)> {
    console_log!("{:#?} {}", &input, &group_location);
    let validation = validate_turnstile(&env, &input.cf_turnstile_response).await;

    if !validation.success {
        if !validation.success {
            return Err((StatusCode::FORBIDDEN, Json(validation)));
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

use axum::response::{IntoResponse, Response};

use octocrab::params::repos::Reference;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct OpenContentPrRequest {
    data: String,
    #[serde(rename(deserialize = "cf-turnstile-response"))]
    cf_turnstile_response: String,
}

#[worker::send]
pub async fn open_content_pr(
    Path((group_location, content_type)): Path<(String, String)>,
    Extension(env): Extension<Env>,
    Form(input): Form<OpenContentPrRequest>,
) -> Response {
    match open_content_pr_inner(&env, &group_location, &content_type, input).await {
        Ok(pr_url) => (
            StatusCode::CREATED,
            Json(json!({
                "ok": true,
                "pr_url": pr_url,
            })),
        )
            .into_response(),

        Err(err) => {
            worker::console_error!("failed to create GitHub PR: {err}");

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "error": "failed to create pull request",
                })),
            )
                .into_response()
        }
    }
}

// EXPLICIT DEPENDENCY: github (and referenced module code)
async fn open_content_pr_inner(
    env: &Env,
    group_location: &str,
    content_type: &str,
    input: OpenContentPrRequest,
) -> Result<Option<String>, String> {
    let validation = validate_turnstile(env, &input.cf_turnstile_response).await;

    if !validation.success {
        if !validation.success {
            return Err("bad turnstile".to_string());
        }
    }
    if !valid_slug(group_location) {
        return Err("invalid group_location".to_string());
    }

    let app_id: u64 = env
        .var("GITHUB_APP_ID")
        .map_err(|e| e.to_string())?
        .to_string()
        .parse()
        .map_err(|e| format!("invalid GITHUB_APP_ID: {e}"))?;

    let installation_id: u64 = env
        .var("GITHUB_INSTALLATION_ID")
        .map_err(|e| e.to_string())?
        .to_string()
        .parse()
        .map_err(|e| format!("invalid GITHUB_INSTALLATION_ID: {e}"))?;

    let owner = env
        .var("GITHUB_OWNER")
        .map_err(|e| e.to_string())?
        .to_string();

    let repo = env
        .var("GITHUB_REPO")
        .map_err(|e| e.to_string())?
        .to_string();

    let base = env
        .var("GITHUB_BASE_BRANCH")
        .map_err(|e| e.to_string())?
        .to_string();

    let private_key = env
        .secret("GITHUB_APP_PRIVATE_KEY")
        .map_err(|e| e.to_string())?
        .to_string();

    let gh = github::github(app_id, installation_id, &private_key)
        .map_err(|e| format!("GitHub auth/client setup: {e}"))?;

    let timestamp = js_sys::Date::now() as u64;

    let branch = format!("{group_location}-{content_type}-{timestamp}");

    let path = format!("{content_type}/{group_location}/{timestamp}.md");

    let base_ref = gh
        .repos(&owner, &repo)
        .get_ref(&Reference::Branch(base.clone()))
        .await
        .map_err(|e| format!("get base branch: {e}"))?;

    let base_sha = match base_ref.object {
        octocrab::models::repos::Object::Commit { sha, .. } => sha,
        _ => {
            return Err("base branch did not point to a commit".to_string());
        }
    };

    gh.repos(&owner, &repo)
        .create_ref(&Reference::Branch(branch.clone()), base_sha)
        .await
        .map_err(|e| format!("create branch: {e:#?}"))?;

    let mut contents = input.data.as_bytes().to_vec();

    contents.push(b'\n');

    gh.repos(&owner, &repo)
        .create_file(
            &path,
            format!("Add content submission for {group_location}-{content_type}"),
            &contents,
        )
        .branch(&branch)
        .send()
        .await
        .map_err(|e| format!("create file: {e}"))?;

    let pr = gh
        .pulls(&owner, &repo)
        .create(
            format!("Content submission: {group_location}-{content_type}"),
            &branch,
            &base,
        )
        .body(format!(
            "Automated content submission for \
             `{group_location}-{content_type}`."
        ))
        .send()
        .await
        .map_err(|e| format!("create pull request: {e}"))?;

    Ok(pr.html_url.map(|url| url.to_string()))
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn parse_public_key(s: &str) -> Option<VerifyingKey> {
    if s.len() != 64 {
        return None;
    }

    let mut bytes = [0u8; 32];

    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }

    VerifyingKey::from_bytes(&bytes).ok()
}

#[worker::send]
async fn discord_interaction(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let signature = headers
        .get("X-Signature-Ed25519")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<Signature>().ok());

    let Some(timestamp) = headers
        .get("X-Signature-Timestamp")
        .and_then(|v| v.to_str().ok())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let Some(signature) = signature else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let Ok(public_key) = env.var("DISCORD_PUBLIC_KEY") else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    let Some(key) = parse_public_key(&public_key.to_string()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    let mut signed = timestamp.as_bytes().to_vec();
    signed.extend_from_slice(&body);

    if key.verify_strict(&signed, &signature).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let Ok(interaction) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    match interaction["type"].as_u64() {
        // Discord endpoint PING
        Some(1) => Json(json!({
            "type": 1
        }))
        .into_response(),

        // Slash command
        Some(2) => {
            let command = interaction["data"]["name"].as_str().unwrap_or("");

            let content = match command {
                "ping" => "pong",
                _ => "unknown command",
            };

            Json(json!({
                "type": 4,
                "data": {
                    "content": content
                }
            }))
            .into_response()
        }

        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}