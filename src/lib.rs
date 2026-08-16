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
use chrono::{DateTime, Duration, Utc};
use http::HeaderMap;
use tower_service::Service;
use wasm_bindgen::JsValue;
use worker::*;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use web_sys::{Blob, BlobPropertyBag, FormData};
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
            "/api/content/{group_location}/articles/pr",
            post(open_article_pr),
        )
        .route(
            "/api/content/{group_location}/events/pr",
            post(open_events_pr),
        )
        .route("/", get(Redirect::permanent("/index.html")))
        .route("/contact", get(Redirect::permanent("/pages/contact")))
        .route("/contact/", get(Redirect::permanent("/pages/contact")))
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

#[derive(Serialize, Deserialize)]
struct FormResponse {
    first_name: String,
    last_name: String,
    email: String,
    phone: String,
    howhear: String,
    comment: String,
    city: String,
}

#[derive(Serialize, Debug)]
struct EmailContacts {
    first_name: String,
    last_name: String,
    email: String,
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
    let Ok(discord_bot_channel) = env.var("DISCORD_BOT_CHANNEL") else {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(validation)));
    };
    let message_res = send_discord_message(
        &env,
        &discord_bot_channel.to_string(),
        &serde_json::to_string(&FormResponse {
            first_name: input.first_name,
            last_name: input.last_name,
            email: input.email,
            phone: input.phone,
            howhear: input.howhear,
            comment: input.comment,
            city: group_location,
        })
        .unwrap(),
    )
    .await;
    if message_res.is_err() {
        console_log!("{:#?}", &message_res.err().unwrap());

        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(validation)));
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

impl OpenContentPrRequest {
    pub fn get_article_header(&self, branch: &str, date: &str) -> String {
        let authors_formatted = self
            .authors
            .split(",")
            .map(|a| "\"".to_owned() + a + "\"")
            .collect::<Vec<String>>()
            .join(", ");
        return format!("+++\ntitle= \"{}\"\ndate = {}\nauthors = [{}]\n[extra]\nimage = \"{}\"\nimage_alt = \"{}\"\nbranch=\"{}\"\n+++\n", self.title, date, authors_formatted, self.image_url, self.image_alt, branch);
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Event {
    title: String,
    date: String,
    image: String,
    image_alt: String,
    location: String
}

#[derive(Debug, Deserialize, Serialize)]
struct Events {
    events: Vec<Event>
}


#[derive(Debug, Deserialize)]
struct OpenEventPrRequest {
    #[serde(rename(deserialize = "cf-turnstile-response"))]
    cf_turnstile_response: String,
    title: String,
    image: String,
    image_alt: String,
    location: String,
    date: String
}

#[derive(Debug, Deserialize)]
struct OpenContentPrRequest {
    data: String,
    #[serde(rename(deserialize = "cf-turnstile-response"))]
    cf_turnstile_response: String,
    title: String,
    authors: String,
    image_url: String,
    image_alt: String,
}

#[worker::send]
pub async fn open_events_pr(
    Path(group_location): Path<String>,
    Extension(env): Extension<Env>,
    Form(input): Form<OpenEventPrRequest>,
) -> Response {
    match open_content_pr_inner(
        &env,
        &group_location,
        "events",
        &serde_json::to_string(&Events{events: vec![Event{ title: input.title, date: input.date, image: input.image, image_alt: input.image_alt, location: input.location }]}).unwrap(),
        &input.cf_turnstile_response,
        "json"
    )
    .await
    {
         Ok(pr_url) => (
            StatusCode::SEE_OTHER,
            Redirect::to(&format!(
                "/pages/articlesubmitted?{}",
                &form_urlencoded::Serializer::new(String::new())
                    .append_pair("pr_url", &pr_url.unwrap_or("".to_owned()))
                    .finish()
            )),
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

#[worker::send]
pub async fn open_article_pr(
    Path(group_location): Path<String>,
    Extension(env): Extension<Env>,
    Form(input): Form<OpenContentPrRequest>,
) -> Response {
    let date = Utc::now().format("%Y-%m-%d").to_string();
    match open_content_pr_inner(
        &env,
        &group_location,
        "articles",
        &(input.get_article_header(&group_location, &date) + &input.data.clone()),
        &input.cf_turnstile_response,
        "md"
    )
    .await
    {
        Ok(pr_url) => (
            StatusCode::SEE_OTHER,
            Redirect::to(&format!(
                "/pages/articlesubmitted?{}",
                &form_urlencoded::Serializer::new(String::new())
                    .append_pair("pr_url", &pr_url.unwrap_or("".to_owned()))
                    .finish()
            )),
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
    content: &str,
    turnstile: &str,
    file_ending: &str
) -> Result<Option<String>, String> {
    let validation = validate_turnstile(env, turnstile).await;

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

    let path = format!("{content_type}/{group_location}/{timestamp}.{file_ending}");

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

    let mut contents = content.as_bytes().to_vec();

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

fn create_contacts_csv(
    bot_user_id: &str,
    group_location: &str,
    messages: Vec<serde_json::Value>,
) -> String {
    let mut writer = csv::Writer::from_writer(vec![]);
    let one_week_ago = Utc::now() - Duration::days(7);
    for message in &messages {
        let author_id = message["author"]["id"].as_str().unwrap_or("");

        if author_id != bot_user_id {
            console_error!("ids neq {author_id} {bot_user_id} ");
            continue;
        }

        let Some(timestamp) = message["timestamp"].as_str() else {
            console_error!("failed to snag timestamp");
            continue;
        };

        let Ok(timestamp) = DateTime::parse_from_rfc3339(timestamp) else {
            console_error!("failed to parse timestamp");

            continue;
        };

        if timestamp < one_week_ago {
            console_error!("timestampt too old");

            continue;
        }

        let content = message["content"].as_str().unwrap_or("");

        let parsed: FormResponse = match serde_json::from_str(content) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if parsed.city != group_location {
            console_error!("skipping neq {} {}", parsed.city, group_location);
            continue;
        }

        let contact = EmailContacts {
            first_name: parsed.first_name,
            last_name: parsed.last_name,
            email: parsed.email,
        };

        console_error!("adding contact {contact:#?}");

        writer.serialize(contact).unwrap();
    }
    String::from_utf8(writer.into_inner().unwrap()).unwrap()
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

    let channel_id = interaction["channel_id"].as_str().unwrap();
    console_error!("running command on channel {channel_id}");

    let Ok(discord_bot_channel) = env.var("DISCORD_BOT_CHANNEL") else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let bot_channel = &discord_bot_channel.to_string();
    console_error!("bot channel set to {bot_channel}");

    if channel_id != bot_channel {
        console_error!("User tried running command from wrong channel");
        return StatusCode::BAD_REQUEST.into_response();
    }

    match interaction["type"].as_u64() {
        Some(1) => Json(json!({
            "type": 1
        }))
        .into_response(),

        Some(2) => {
            let command = interaction["data"]["name"].as_str().unwrap_or("");
            let argument = interaction["data"]["options"]
                .as_array()
                .and_then(|options| options.first())
                .and_then(|option| option["value"].as_str())
                .unwrap_or("");
            if command != "report" || argument == "" {
                console_error!("bad command {command} {argument}");
                return StatusCode::BAD_REQUEST.into_response();
            }

            let msgs = get_all_discord_messages(&env, bot_channel).await;

            let Ok(msgs) = msgs else {
                return StatusCode::BAD_REQUEST.into_response();
            };

            console_error!("msgs {msgs:#?}");

            let Ok(bot_user_id) = env.var("DISCORD_BOT_ID") else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };

            let csv = create_contacts_csv(&bot_user_id.to_string(), argument, msgs);

            let res_json = json!({
                "type": 4,
                "data": {
                    "content": "Contacts export from last 7 days",
                    "attachments": [
                        {
                            "id": 0,
                            "filename": "export.csv"
                        }
                    ]
                }
            })
            .to_string();
            console_error!("csv {csv}");
            let form = FormData::new()
                .map_err(|e| worker::Error::RustError(format!("{e:?}")))
                .unwrap();
            form.append_with_str("payload_json", &res_json)
                .map_err(|e| worker::Error::RustError(format!("{e:?}")))
                .unwrap();

            let parts = js_sys::Array::new();
            parts.push(&JsValue::from_str(&csv));

            let options = BlobPropertyBag::new();
            options.set_type("text/csv; charset=utf-8");

            let blob = Blob::new_with_str_sequence_and_options(&parts.into(), &options)
                .map_err(|e| worker::Error::RustError(format!("{e:?}")))
                .unwrap();

            form.append_with_blob_and_filename("files[0]", &blob, "export.csv")
                .map_err(|e| worker::Error::RustError(format!("{e:?}")))
                .unwrap();
            let k = worker::web_sys::Response::new_with_opt_form_data(Some(&form))
                .map_err(|e| worker::Error::RustError(format!("{e:?}")))
                .unwrap()
                .into();
            let worker_response = worker::response_from_wasm(k).unwrap();

            let (parts, body) = worker_response.into_parts();

            let response = axum::http::Response::from_parts(parts, axum::body::Body::new(body));
            response
        }

        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}

async fn send_discord_message(env: &Env, channel_id: &str, content: &str) -> worker::Result<()> {
    let token = env.secret("DISCORD_BOT_TOKEN")?.to_string();

    let headers = Headers::new();
    headers.set("Authorization", &format!("Bot {token}"))?;
    headers.set("Content-Type", "application/json")?;

    let body = serde_json::json!({
        "content": content,
        "allowed_mentions": {
            "parse": []
        }
    })
    .to_string();

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body)));

    let req = Request::new_with_init(
        &format!("https://discord.com/api/v10/channels/{channel_id}/messages"),
        &init,
    )?;

    let response = Fetch::Request(req).send().await?;

    if !(200..300).contains(&response.status_code()) {
        return Err(worker::Error::RustError(format!(
            "Discord returned {}",
            response.status_code()
        )));
    }

    Ok(())
}

async fn get_all_discord_messages(
    env: &Env,
    channel_id: &str,
) -> worker::Result<Vec<serde_json::Value>> {
    let token = env.secret("DISCORD_BOT_TOKEN")?.to_string();

    let mut all_messages = Vec::new();
    let mut before: Option<String> = None;

    loop {
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bot {token}"))?;

        let mut url =
            format!("https://discord.com/api/v10/channels/{channel_id}/messages?limit=100");

        if let Some(before_id) = &before {
            url.push_str(&format!("&before={before_id}"));
        }

        let mut init = RequestInit::new();
        init.with_method(Method::Get).with_headers(headers);

        let req = Request::new_with_init(&url, &init)?;

        let mut response = Fetch::Request(req).send().await?;

        if !(200..300).contains(&response.status_code()) {
            return Err(worker::Error::RustError(format!(
                "Discord returned {}",
                response.status_code()
            )));
        }

        let messages: Vec<serde_json::Value> = response.json().await?;

        if messages.is_empty() {
            break;
        }

        // Discord returns newest -> oldest.
        // The last message is therefore our next pagination cursor.
        before = messages
            .last()
            .and_then(|message| message["id"].as_str())
            .map(String::from);

        let count = messages.len();

        all_messages.extend(messages);

        // Less than 100 means we've reached the beginning.
        if count < 100 {
            break;
        }

        if before.is_none() {
            break;
        }
    }

    Ok(all_messages)
}
