//! The one HTTP call `rivet sync` makes per request.
//!
//! Both providers describe a request as a plain value ([`Request`]) and
//! return a page ([`Page`]), so request assembly and response parsing are
//! pure functions a unit test pins offline. This module turns the request
//! value into one reqwest call; only [`send`] touches the network.

use std::time::Duration;

/// How long one tracker request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One HTTP request, described without a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Request {
    /// The HTTP method.
    pub(super) method: &'static str,
    /// The absolute URL.
    pub(super) url: String,
    /// A bearer token, when the tracker takes one.
    pub(super) bearer: Option<String>,
    /// A raw `Authorization` value, when the tracker takes one.
    pub(super) authorization: Option<String>,
    /// The JSON body, when the request carries one.
    pub(super) body: Option<String>,
}

impl Request {
    /// A bodyless `GET`.
    pub(super) fn get(url: String) -> Request {
        Request {
            method: "GET",
            url,
            bearer: None,
            authorization: None,
            body: None,
        }
    }

    /// A `POST` with a JSON body.
    pub(super) fn post(url: String, body: String) -> Request {
        Request {
            method: "POST",
            url,
            bearer: None,
            authorization: None,
            body: Some(body),
        }
    }

    /// Send a bearer token.
    pub(super) fn bearer(mut self, token: &str) -> Request {
        self.bearer = Some(token.to_string());
        self
    }

    /// Send a raw `Authorization` value, with no scheme in front of it.
    pub(super) fn authorization(mut self, value: &str) -> Request {
        self.authorization = Some(value.to_string());
        self
    }
}

/// One page of tracker issues, and the token that reads the next page.
///
/// `next` is `None` on the last page, so the caller reads the tracker to the
/// end instead of treating a full page as the whole tracker — which would
/// report every later story as missing and let `--apply` file duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Page {
    /// The issues on this page.
    pub(super) issues: Vec<super::issue::Issue>,
    /// The token that reads the next page, when one exists.
    pub(super) next: Option<String>,
}

/// Send one request and return the response body.
///
/// The failure is a plain message: the caller decides whether the failed
/// request was a read (`E3020`) or a write (`E3021`).
pub(super) async fn send(request: &Request) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|err| format!("failed to build the HTTP client: {err}"))?;
    let method = reqwest::Method::from_bytes(request.method.as_bytes())
        .map_err(|err| format!("`{}` is not an HTTP method: {err}", request.method))?;
    let mut builder = client.request(method, &request.url);
    if let Some(token) = &request.bearer {
        builder = builder.bearer_auth(token);
    }
    if let Some(value) = &request.authorization {
        builder = builder.header(reqwest::header::AUTHORIZATION, value);
    }
    if let Some(body) = &request.body {
        builder = builder
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.clone());
    }

    let response = builder
        .send()
        .await
        .map_err(|err| format!("the tracker request to {} failed: {err}", request.url))?;
    let status = response.status();
    let body = response.text().await.map_err(|err| {
        format!(
            "the tracker answered {} with an unreadable body: {err}",
            status.as_u16()
        )
    })?;
    if !status.is_success() {
        return Err(format!("the tracker answered {}: {body}", status.as_u16()));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_builder_carries_its_method_url_and_headers() {
        let request = Request::post("https://example.test/graphql".to_string(), "{}".to_string())
            .authorization("key");
        assert_eq!(request.method, "POST");
        assert_eq!(request.authorization.as_deref(), Some("key"));
        assert!(request.bearer.is_none());

        let request = Request::get("https://example.test/x".to_string()).bearer("token");
        assert_eq!(request.method, "GET");
        assert_eq!(request.bearer.as_deref(), Some("token"));
        assert!(request.body.is_none());
    }
}
