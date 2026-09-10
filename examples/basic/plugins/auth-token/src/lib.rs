//! Bearer-token plugin for Rivet servers.
//!
//! The plugin serves `GET /auth/check`, which reports whether the request
//! carries the token the deployment configured. A request authenticates only
//! when the process set `RIVET_AUTH_TOKEN`, the request carries
//! `Authorization: Bearer <token>`, and the presented token matches the
//! configured one.
//!
//! The plugin reads the environment once, at install time. A running server
//! therefore keeps the token it started with, no request reads the
//! environment, and the tests drive every decision through an [`AuthConfig`]
//! value instead of mutating process env.

use axum::Json;
use axum::Router;
use axum::extract::Extension;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::routing::get;
use serde::Serialize;

/// The environment variable that holds the configured bearer token.
const TOKEN_ENV: &str = "RIVET_AUTH_TOKEN";

/// The credential scheme this plugin requires and reports.
const SCHEME: &str = "bearer";

/// The exact scheme prefix an `Authorization` header must carry.
const BEARER_PREFIX: &str = "Bearer ";

/// The plugin's install-time view of the token configuration.
///
/// `None`, and `Some` holding an empty string, both mean "no token
/// configured". [`AuthConfig::token`] is the one place that resolves either
/// shape into a usable secret.
#[derive(Clone)]
struct AuthConfig {
    /// The token read from the environment at install time.
    token: Option<String>,
}

impl AuthConfig {
    /// Read the token from the environment, once, at install time.
    ///
    /// An unset variable yields no token. A variable set to invalid UTF-8
    /// yields no token as well, because the plugin compares the credential as
    /// a UTF-8 string.
    fn from_env() -> Self {
        Self {
            token: std::env::var(TOKEN_ENV).ok(),
        }
    }

    /// The usable token, or `None` when the deployment configured none.
    ///
    /// An empty token is no token: it would otherwise match the empty bearer
    /// credential that a client sends as `Authorization: Bearer `.
    fn token(&self) -> Option<&str> {
        self.token.as_deref().filter(|token| !token.is_empty())
    }
}

/// Decide whether the presented credential authenticates a request.
///
/// `presented` is the raw `Authorization` header value. The function answers
/// true only when a token is configured, the header carries the exact
/// `Bearer ` prefix, and the two token strings match. The scheme match is
/// case-sensitive on purpose: this plugin advertises one spelling, so a client
/// sends that spelling.
fn authenticates(config: &AuthConfig, presented: Option<&str>) -> bool {
    let (Some(configured), Some(presented)) = (config.token(), presented) else {
        return false;
    };
    let Some(token) = presented.strip_prefix(BEARER_PREFIX) else {
        return false;
    };
    constant_time_eq(token.as_bytes(), configured.as_bytes())
}

/// Compare two byte slices without stopping at the first differing byte.
///
/// The loop XORs every byte pair and accumulates the result into one byte, so
/// the running time follows the slice length and not the position of the first
/// mismatch. The length check before the loop still reveals the length of the
/// configured token through timing; that length is not sensitive here, because
/// the token is an operator-chosen credential of fixed shape, not a secret
/// whose strength depends on hiding its size.
fn constant_time_eq(presented: &[u8], configured: &[u8]) -> bool {
    if presented.len() != configured.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in presented.iter().zip(configured.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

/// The body of a `GET /auth/check` response.
#[derive(Serialize)]
struct CheckResponse {
    /// Whether the request carried the configured token.
    authenticated: bool,
    /// The credential scheme this plugin requires.
    scheme: &'static str,
}

/// Answer `GET /auth/check` from the install-time configuration.
async fn check(
    Extension(config): Extension<AuthConfig>,
    headers: HeaderMap,
) -> Json<CheckResponse> {
    let presented = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    Json(CheckResponse {
        authenticated: authenticates(&config, presented),
        scheme: SCHEME,
    })
}

/// Build the plugin's router around an already resolved configuration.
///
/// The configuration travels as an axum extension, so the handler reads the
/// value that install time computed and never touches the environment.
fn router_with(config: AuthConfig) -> Router {
    Router::new()
        .route("/auth/check", get(check))
        .layer(Extension(config))
}

/// The bearer-token plugin, which an application composes at build time.
///
/// The type carries no state. Every value the plugin needs comes from the
/// environment when [`AuthConfig::from_env`] runs during installation.
pub struct Plugin;

impl rivet_plugin_api::Plugin for Plugin {
    fn name(&self) -> &'static str {
        "auth-token"
    }

    /// Merge `GET /auth/check` into `router`, reading the token now.
    fn install(self, router: Router) -> Router {
        router.merge(router_with(AuthConfig::from_env()))
    }
}

/// Serve `GET /auth/check` from `router`.
///
/// This is the crate entry point: an application composes the plugin without
/// naming its type. The call routes through [`rivet_plugin_api::install`], so
/// the deployment logs the composition, and the token is read here, once.
pub fn install(router: Router) -> Router {
    rivet_plugin_api::install(router, Plugin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use tower::ServiceExt;

    /// The token every configured test case uses.
    const SECRET: &str = "correct-horse-battery-staple";

    /// A configuration that holds a usable token.
    fn configured() -> AuthConfig {
        AuthConfig {
            token: Some(SECRET.to_string()),
        }
    }

    /// A configuration without a token.
    fn unconfigured() -> AuthConfig {
        AuthConfig { token: None }
    }

    /// Call `GET /auth/check` on `router`, with `authorization` when given.
    ///
    /// Returns the status and the parsed JSON body.
    async fn call_check(router: Router, authorization: Option<&str>) -> (StatusCode, Value) {
        let mut request = Request::builder().uri("/auth/check");
        if let Some(value) = authorization {
            request = request.header(AUTHORIZATION, value);
        }
        let request = request.body(Body::empty()).expect("build the request");
        let response = router.oneshot(request).await.expect("call the router");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read the response body");
        let json = serde_json::from_slice(&body).expect("parse the response body as JSON");
        (status, json)
    }

    #[test]
    fn correct_token_authenticates() {
        assert!(authenticates(
            &configured(),
            Some("Bearer correct-horse-battery-staple")
        ));
    }

    #[test]
    fn wrong_token_does_not_authenticate() {
        assert!(!authenticates(&configured(), Some("Bearer wrong")));
        assert!(!authenticates(
            &configured(),
            Some("Bearer correct-horse-battery-stapl")
        ));
        assert!(!authenticates(
            &configured(),
            Some("Bearer correct-horse-battery-staple ")
        ));
    }

    #[test]
    fn missing_header_does_not_authenticate() {
        assert!(!authenticates(&configured(), None));
    }

    #[test]
    fn wrong_scheme_does_not_authenticate() {
        assert!(!authenticates(
            &configured(),
            Some("Basic correct-horse-battery-staple")
        ));
        assert!(!authenticates(
            &configured(),
            Some("bearer correct-horse-battery-staple")
        ));
        assert!(!authenticates(&configured(), Some(SECRET)));
    }

    #[test]
    fn empty_configured_token_does_not_authenticate() {
        let config = AuthConfig {
            token: Some(String::new()),
        };
        assert!(!authenticates(&config, Some("Bearer ")));
        assert!(!authenticates(&config, Some(SECRET)));
    }

    #[test]
    fn unconfigured_plugin_does_not_authenticate() {
        assert!(!authenticates(&unconfigured(), Some(SECRET)));
        assert!(!authenticates(
            &unconfigured(),
            Some("Bearer correct-horse-battery-staple")
        ));
    }

    #[tokio::test]
    async fn check_route_reports_a_configured_match() {
        let (status, body) = call_check(
            router_with(configured()),
            Some("Bearer correct-horse-battery-staple"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["authenticated"], Value::Bool(true));
        assert_eq!(body["scheme"], Value::String(SCHEME.to_string()));
        assert_eq!(body.as_object().map(|object| object.len()), Some(2));
    }

    #[tokio::test]
    async fn check_route_reports_no_match_from_a_wrong_token() {
        let (status, body) = call_check(router_with(configured()), Some("Bearer wrong")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["authenticated"], Value::Bool(false));
        assert_eq!(body["scheme"], Value::String(SCHEME.to_string()));
    }

    #[tokio::test]
    async fn check_route_reports_no_match_when_unconfigured() {
        let (status, body) = call_check(router_with(unconfigured()), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["authenticated"], Value::Bool(false));
        assert_eq!(body["scheme"], Value::String(SCHEME.to_string()));
        assert_eq!(body.as_object().map(|object| object.len()), Some(2));
    }

    #[test]
    fn plugin_name_is_stable() {
        assert_eq!(rivet_plugin_api::Plugin::name(&Plugin), "auth-token");
    }

    #[tokio::test]
    async fn plugin_install_mounts_the_check_route() {
        let router = rivet_plugin_api::install(Router::new(), Plugin);
        let (status, body) = call_check(router, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["scheme"], Value::String(SCHEME.to_string()));
    }
}
