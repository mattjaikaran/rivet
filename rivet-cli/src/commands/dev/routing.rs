//! Where each request path goes: the Rust backend or the frontend dev server.
//!
//! The proxy owns two path spaces. The blueprint keeps its own route paths,
//! and `/api` is the proxy's escape hatch for backend paths outside the
//! blueprint. The generated backend serves no prefix of its own, so the proxy
//! rewrites `/api/*` to `/*` on the way upstream. A path the blueprint
//! declares wins over the prefix, so a blueprint route that itself starts
//! with `/api` still resolves.

use super::DevTopology;
use axum::http::Uri;

/// The route prefix the proxy keeps for the Rust backend.
pub(crate) const API_PREFIX: &str = "/api";

/// The origin that owns `path`: the backend for its own routes and the API
/// prefix, the frontend for everything else. With no frontend, the backend
/// owns every path.
pub(crate) fn upstream_for(topology: &DevTopology, path: &str) -> String {
    match &topology.frontend {
        Some(frontend) if !backend_owns(topology, path) => frontend.clone(),
        _ => topology.backend.clone(),
    }
}

/// Whether the Rust backend owns this path.
pub(crate) fn backend_owns(topology: &DevTopology, path: &str) -> bool {
    is_declared_route(topology, path) || is_api_path(path)
}

/// Whether the blueprint declares this path.
fn is_declared_route(topology: &DevTopology, path: &str) -> bool {
    topology
        .routes
        .iter()
        .any(|route| route_matches(path, route))
}

/// Whether the proxy reserves this path for the backend.
fn is_api_path(path: &str) -> bool {
    path == API_PREFIX
        || path
            .strip_prefix(API_PREFIX)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The path the backend receives for a request path the backend owns.
///
/// The backend serves the blueprint's own paths, so the proxy strips the
/// `/api` prefix it added. A declared route keeps its path verbatim.
pub(crate) fn upstream_path<'a>(topology: &DevTopology, path: &'a str) -> &'a str {
    if is_declared_route(topology, path) || !is_api_path(path) {
        return path;
    }
    let rest = &path[API_PREFIX.len()..];
    if rest.is_empty() { "/" } else { rest }
}

/// The URL to request from `origin`, query string included and the path
/// rewritten for the backend.
pub(crate) fn upstream_url(topology: &DevTopology, origin: &str, uri: &Uri) -> String {
    let path = upstream_path(topology, uri.path());
    match uri.query() {
        Some(query) => format!("{origin}{path}?{query}"),
        None => format!("{origin}{path}"),
    }
}

/// Match a request path against a declared route the way axum does: the
/// segments must line up, and a `{name}` segment accepts any value.
fn route_matches(path: &str, route: &str) -> bool {
    let declared: Vec<&str> = route.split('/').collect();
    let requested: Vec<&str> = path.split('/').collect();
    declared.len() == requested.len()
        && declared
            .iter()
            .zip(&requested)
            .all(|(declared, requested)| {
                (declared.starts_with('{') && declared.ends_with('}')) || declared == requested
            })
}

#[cfg(test)]
mod tests;
