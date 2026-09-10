//! The embedded frontend build: `dist/` compiled into the generated binary.
//!
//! `rivet dev` proxies to a frontend dev server; the built binary has no
//! sidecar files at all. When the project configures `[frontend] dist`, this
//! module renders the `mod assets` block that compiles that directory into
//! the binary with `rust-embed` and serves it from the router's fallback, so
//! a blueprint route always wins over an asset path (pillar 03).
//!
//! The rendered module is a template with `@@NAME@@` tokens instead of a
//! `format!` string: the asset code is mostly braces, and the tokens keep it
//! readable as Rust.

use crate::config::RivetConfig;
use std::path::{Path, PathBuf};

/// What `rivet build` did with the project's static assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetEmbedding {
    /// The project configures no `[frontend] dist` directory.
    None,
    /// The configured directory does not exist, so nothing was embedded.
    Missing(PathBuf),
    /// The directory is compiled into the binary.
    Embedded {
        /// The directory on disk, for the build report.
        dir: PathBuf,
        /// The path `rust-embed` resolves against the generated crate.
        folder: String,
    },
}

/// Resolve the assets for a project: the configured `[frontend] dist`
/// directory, when it exists.
pub(super) fn resolve(config: &RivetConfig, project_dir: &Path) -> AssetEmbedding {
    let Some(dist) = config.frontend.dist.as_deref() else {
        return AssetEmbedding::None;
    };
    let dir = project_dir.join(dist);
    if !dir.is_dir() {
        return AssetEmbedding::Missing(dir);
    }
    AssetEmbedding::Embedded {
        folder: embed_folder(Path::new(dist)),
        dir,
    }
}

/// The `#[folder]` value for an asset directory.
///
/// `rust-embed` resolves a relative folder against the manifest directory of
/// the crate it compiles, and the generated crate lives in `generated/`, one
/// level below the project. An absolute directory is used as it is.
fn embed_folder(dist: &Path) -> String {
    let dist = dist.strip_prefix(".").unwrap_or(dist);
    Path::new("..").join(dist).to_string_lossy().into_owned()
}

/// The router line that mounts the embedded build as the fallback.
const FALLBACK: &str = "\n        .fallback(assets::serve)";

/// The line that compresses every response, applied after the plugins
/// install so their routes are compressed too.
///
/// Compression on the wire costs no binary size: the assets stay
/// uncompressed inside the binary, and one pass per request compresses the
/// response. `rust-embed`'s own `compression` feature would trade binary
/// size instead, and this phase does not measure that trade.
const COMPRESSION: &str =
    "\n    let app = app.layer(tower_http::compression::CompressionLayer::new());\n";

/// The generated asset wiring for `main.rs`.
///
/// Every field is empty when the app embeds no assets.
pub(super) struct Wiring {
    /// The `mod assets` block.
    pub(super) module: String,
    /// The router's fallback line.
    pub(super) fallback: String,
    /// The compression layer, applied after the plugins.
    pub(super) compression: String,
}

/// Render the generated asset wiring.
///
/// `spa` adds the single-page fallback: a browser navigation to a path that
/// names no file reaches the embedded `index.html`.
pub(super) fn render(embedding: &AssetEmbedding, spa: bool) -> Wiring {
    let AssetEmbedding::Embedded { folder, .. } = embedding else {
        return Wiring {
            module: String::new(),
            fallback: String::new(),
            compression: String::new(),
        };
    };
    let fallback = if spa { SPA_FALLBACK } else { "" };
    Wiring {
        module: MODULE
            .replace("@@FOLDER@@", &super::rust_str(folder))
            .replace("@@SPA@@", fallback),
        fallback: FALLBACK.to_string(),
        compression: COMPRESSION.to_string(),
    }
}

/// The single-page fallback appended to the asset lookup.
const SPA_FALLBACK: &str = r#"
            .or_else(|| {
                if wants_index(&method, &uri, &headers) {
                    Frontend::get("index.html")
                } else {
                    None
                }
            })"#;

/// The asset module every embedding app carries.
const MODULE: &str = r#"/// The embedded frontend build (pillar 03).
///
/// `rust-embed` compiles the directory into the binary, and `debug-embed`
/// keeps that true for debug builds too, so no build reads the directory at
/// run time. The router calls `serve` only when no route matched, so a
/// blueprint route always wins.
mod assets {
    use axum::body::Body;
    use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
    use axum::response::Response;
    use percent_encoding::percent_decode_str;
    use rust_embed::{EmbeddedFile, RustEmbed};

    /// The frontend build, embedded at compile time.
    #[derive(RustEmbed)]
    #[folder = @@FOLDER@@]
    struct Frontend;

    /// Serve one request from the embedded build.
    pub(super) async fn serve(method: Method, uri: Uri, headers: HeaderMap) -> Response {
        if method != Method::GET && method != Method::HEAD {
            let mut response = plain(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
            response
                .headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
            return response;
        }
        let Some(relative) = relative_path(uri.path()) else {
            return plain(StatusCode::NOT_FOUND, "not found");
        };
        let file = resolve(&relative)@@SPA@@;
        match file {
            Some(file) => serve_file(method, file, &headers),
            None => plain(StatusCode::NOT_FOUND, "not found"),
        }
    }

    /// The embedded file for a path inside the folder: the file itself, or a
    /// directory's own `index.html`.
    fn resolve(relative: &str) -> Option<EmbeddedFile> {
        if relative.is_empty() || relative.ends_with('/') {
            return Frontend::get(&format!("{relative}index.html"));
        }
        Frontend::get(relative)
    }

    /// Whether the request wants the single-page entry point: a browser
    /// navigation to a path that names no file.
    ///
    /// This mirrors the frontend dev server's own rewrite rule. An `XHR` to a
    /// missing backend path, and a missing asset that carries an extension,
    /// keep the `404` they ask for instead of receiving an HTML page.
    fn wants_index(method: &Method, uri: &Uri, headers: &HeaderMap) -> bool {
        let last = uri.path().rsplit('/').next().unwrap_or_default();
        (method == &Method::GET || method == &Method::HEAD)
            && !last.contains('.')
            && headers
                .get(header::ACCEPT)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("text/html"))
    }

    /// The embedded path for a request path: percent-decoded, without the
    /// leading slash, or `None` when the path tries to leave the folder.
    fn relative_path(path: &str) -> Option<String> {
        let decoded = percent_decode_str(path).decode_utf8().ok()?;
        if decoded.split('/').any(|segment| segment == "..") {
            return None;
        }
        Some(decoded.trim_start_matches('/').to_string())
    }

    /// The `200` or `304` for an embedded file, with its cache headers.
    fn serve_file(method: Method, file: EmbeddedFile, request: &HeaderMap) -> Response {
        let EmbeddedFile { data, metadata } = file;
        let length = data.len();
        let etag = HeaderValue::from_str(&etag(metadata.sha256_hash()))
            .unwrap_or_else(|_| HeaderValue::from_static("\"\""));
        let fresh = request
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(',')
                    .any(|candidate| candidate.trim().as_bytes() == etag.as_bytes())
            });

        let mut response = Response::new(if method == Method::HEAD || fresh {
            Body::empty()
        } else {
            Body::from(data)
        });
        *response.status_mut() = if fresh {
            StatusCode::NOT_MODIFIED
        } else {
            StatusCode::OK
        };
        let headers = response.headers_mut();
        if !fresh {
            let mimetype = HeaderValue::from_str(metadata.mimetype())
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
            headers.insert(header::CONTENT_TYPE, mimetype);
            if method == Method::HEAD
                && let Ok(length) = HeaderValue::from_str(&length.to_string())
            {
                headers.insert(header::CONTENT_LENGTH, length);
            }
        }
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=0, must-revalidate"),
        );
        headers.insert(header::ETAG, etag);
        response
    }

    /// A quoted strong ETag from a file hash.
    fn etag(hash: [u8; 32]) -> String {
        use std::fmt::Write as _;
        let mut value = String::with_capacity(34);
        value.push('"');
        for byte in &hash[..16] {
            let _ = write!(value, "{byte:02x}");
        }
        value.push('"');
        value
    }

    /// A plain-text response.
    fn plain(status: StatusCode, message: &'static str) -> Response {
        let mut response = Response::new(Body::from(message));
        *response.status_mut() = status;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        response
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_is_relative_to_the_generated_crate() {
        assert_eq!(embed_folder(Path::new("dist")), "../dist");
        assert_eq!(embed_folder(Path::new("./dist")), "../dist");
        assert_eq!(embed_folder(Path::new("frontend/dist")), "../frontend/dist");
        assert_eq!(
            embed_folder(Path::new("../frontend/dist")),
            "../../frontend/dist"
        );
        assert_eq!(embed_folder(Path::new("/srv/app/dist")), "/srv/app/dist");
    }

    #[test]
    fn no_configured_directory_embeds_nothing() {
        let config = RivetConfig::default();
        assert_eq!(
            resolve(&config, Path::new("/project")),
            AssetEmbedding::None,
            "an absent `[frontend] dist` embeds nothing"
        );
    }

    #[test]
    fn a_missing_directory_is_reported_not_embedded() {
        let dir = crate::test_support::ScratchDir::new("assets-missing");
        let mut config = RivetConfig::default();
        config.frontend.dist = Some("dist".to_string());

        assert_eq!(
            resolve(&config, &dir),
            AssetEmbedding::Missing(dir.join("dist"))
        );
    }

    #[test]
    fn an_existing_directory_is_embedded() {
        let dir = crate::test_support::ScratchDir::new("assets-present");
        std::fs::create_dir_all(dir.join("dist")).expect("create dist");
        let mut config = RivetConfig::default();
        config.frontend.dist = Some("dist".to_string());

        assert_eq!(
            resolve(&config, &dir),
            AssetEmbedding::Embedded {
                dir: dir.join("dist"),
                folder: "../dist".to_string(),
            }
        );
    }

    /// The embedded directory every render test uses.
    fn embedded() -> AssetEmbedding {
        AssetEmbedding::Embedded {
            dir: PathBuf::from("dist"),
            folder: "../dist".to_string(),
        }
    }

    #[test]
    fn the_module_embeds_the_folder_and_mounts_the_fallback() {
        let wiring = render(&embedded(), true);
        assert!(
            wiring.module.contains("#[folder = \"../dist\"]"),
            "{}",
            wiring.module
        );
        assert!(wiring.module.contains("mod assets {"), "{}", wiring.module);
        assert!(
            wiring
                .module
                .contains("/// Serve one request from the embedded build.")
        );
        assert_eq!(wiring.fallback, "\n        .fallback(assets::serve)");
        assert_eq!(
            wiring.compression,
            "\n    let app = app.layer(tower_http::compression::CompressionLayer::new());\n"
        );
    }

    #[test]
    fn client_side_routing_falls_back_only_for_a_navigation() {
        let module = render(&embedded(), true).module;
        assert!(
            module.contains("if wants_index(&method, &uri, &headers)"),
            "{module}"
        );
        assert!(module.contains("Frontend::get(\"index.html\")"), "{module}");
        // The rewrite rule the frontend dev server uses: no extension, and
        // the client accepts HTML.
        assert!(module.contains("!last.contains('.')"), "{module}");
        assert!(module.contains("value.contains(\"text/html\")"), "{module}");
    }

    #[test]
    fn a_project_without_client_side_routing_serves_only_real_assets() {
        let module = render(&embedded(), false).module;
        assert!(!module.contains(".or_else(|| {"), "{module}");
        assert!(!module.contains("wants_index(&method"), "{module}");
        assert!(module.contains("resolve(&relative);"), "{module}");
    }

    #[test]
    fn an_app_without_assets_renders_no_wiring() {
        for embedding in [
            AssetEmbedding::None,
            AssetEmbedding::Missing(PathBuf::from("dist")),
        ] {
            let wiring = render(&embedding, true);
            assert!(wiring.module.is_empty());
            assert!(wiring.fallback.is_empty());
            assert!(wiring.compression.is_empty());
        }
    }
}
