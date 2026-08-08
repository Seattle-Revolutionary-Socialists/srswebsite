use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use http::{
    header::{ACCEPT, USER_AGENT},
    HeaderValue, Request, Response, Uri,
};

use jsonwebtoken::EncodingKey;

use octocrab::{
    auth::AppAuth,
    service::middleware::{
        base_uri::BaseUriLayer,
        extra_headers::ExtraHeadersLayer,
    },
    AuthState, OctoBody, Octocrab, OctocrabBuilder,
};

use tower::Service;

use worker::{
    send::IntoSendFuture,
    Fetch,
};

#[derive(Debug, Default, Clone, Copy)]
struct WorkerFetch;

impl Service<Request<OctoBody>> for WorkerFetch {
    type Response = Response<worker::Body>;
    type Error = worker::Error;

    type Future = Pin<
        Box<
            dyn Future<
                    Output = Result<Self::Response, Self::Error>,
                > + Send
                + 'static,
        >,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(
        &mut self,
        req: Request<OctoBody>,
    ) -> Self::Future {
        let fut = async move {
            // http::Request<OctoBody> -> worker::Request
            let req: worker::Request = req.try_into()?;

            // Cloudflare Fetch
            let response = Fetch::Request(req).send().await?;

            // worker::Response ->
            // http::Response<worker::Body>
            let response: worker::HttpResponse =
                response.try_into()?;

            Ok(response)
        };

        // Worker JS futures are !Send, while Tower requires Send.
        Box::pin(fut.into_send())
    }
}

/// Build an Octocrab client authenticated as this GitHub App
/// installation.
///
/// Octocrab will automatically:
///
///   GitHub App private key
///       -> JWT
///       -> installation token
///       -> cached installation token
///
pub fn github(
    app_id: u64,
    installation_id: u64,
    private_key_pem: &str,
) -> Result<Octocrab, Box<dyn std::error::Error>> {
    let key =
        EncodingKey::from_rsa_pem(private_key_pem.as_bytes())?;

    let app_auth = AppAuth {
        app_id: app_id.into(),
        key,
    };

    let headers = Arc::new(vec![
        (
            USER_AGENT,
            HeaderValue::from_static("contact-pr-worker"),
        ),
        (
            ACCEPT,
            HeaderValue::from_static(
                "application/vnd.github+json",
            ),
        ),
    ]);

    // Octocrab uses a Tower Buffer internally and needs an
    // executor for its background worker.
    //
    // On wasm32 we can spawn it onto the JS event loop.
    let executor = move |
        future: Pin<Box<dyn Future<Output = ()>>>
    | {
        wasm_bindgen_futures::spawn_local(future);
    };

    let app = OctocrabBuilder::new_empty()
        .with_service(WorkerFetch)
        .with_layer(&ExtraHeadersLayer::new(headers))
        .with_layer(&BaseUriLayer::new(
            Uri::from_static("https://api.github.com"),
        ))
        .with_executor(Box::new(executor))
        .with_auth(AuthState::App(app_auth))
        .build()
        .expect("custom Octocrab builder is infallible");

    Ok(app.installation(installation_id.into())?)
}