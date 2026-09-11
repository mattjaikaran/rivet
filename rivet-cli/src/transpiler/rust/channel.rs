//! The internal channel: one typed method per route, over two transports.
//!
//! The generated app reaches its own service layer through `Channel`. The
//! configured topology decides which implementation the compiler sees:
//! `InProcess` monomorphizes to a direct call, and `Grpc` sends the same
//! payload over a gRPC channel that the app also serves. Both are concrete
//! types, so nothing in the request path looks a transport up.
//!
//! Every method here carries a user-chosen name, so each one takes
//! `#[allow(non_snake_case)]`: the DSL decides the name, and a cargo style
//! lint against generated code is noise the user cannot act on. The front end
//! owns identifier policy.

use super::Codegen;
use super::service::render_route;
use crate::config::TransportMode;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::ServiceBlueprint;
use rivet_core::reserved;

/// The gRPC service name; the client path is `/rivet.Channel/<method>`.
const SERVICE_NAME: &str = "rivet.Channel";

/// One route as the channel sees it: the method name plus its shape.
struct Method {
    name: String,
    /// Path parameter `(name, Rust type)` pairs, in path order.
    path_params: Vec<(String, String)>,
    /// Query parameter `(name, Rust type)` pairs, in declaration order.
    query_params: Vec<(String, String)>,
    /// The JSON-body parameter `(name, Rust type)`, when the route takes one.
    request: Option<(String, String)>,
    return_ty: String,
}

impl Method {
    /// The full parameter list without a leading separator.
    fn params(&self) -> String {
        let mut params: Vec<String> = self
            .path_params
            .iter()
            .chain(self.query_params.iter())
            .map(|(name, ty)| format!("{name}: {ty}"))
            .collect();
        if let Some((name, ty)) = &self.request {
            params.push(format!("{name}: {ty}"));
        }
        params.join(", ")
    }

    /// The argument list for a call into `service`: the path variables, then
    /// the query variables, then the body variable. A bodyless path-param
    /// route is `get_order(id)`, never `get_order()`.
    fn call_args(&self) -> String {
        let mut args: Vec<String> = self
            .path_params
            .iter()
            .chain(self.query_params.iter())
            .map(|(name, _)| name.clone())
            .collect();
        if let Some((name, _)) = &self.request {
            args.push(name.clone());
        }
        args.join(", ")
    }

    /// The ordered argument `(name, Rust type)` pairs: path params, then
    /// query params, then the body.
    fn arguments(&self) -> Vec<(String, String)> {
        let mut args: Vec<(String, String)> = self.path_params.clone();
        args.extend(self.query_params.iter().cloned());
        if let Some(request) = &self.request {
            args.push(request.clone());
        }
        args
    }

    /// The channel's return type: the route's type, or the unit.
    fn channel_return(&self) -> &str {
        if self.return_ty.is_empty() {
            "()"
        } else {
            &self.return_ty
        }
    }
}

/// Render `mod channel` for the configured topology.
///
/// The trait and the in-process transport are always emitted, because they
/// are the app's own route table. The gRPC transport appears only when the
/// project selects it, so the other topology carries no unused code.
pub(super) fn render_channel_module(
    codegen: &Codegen<'_>,
    blueprint: &ServiceBlueprint,
    mode: TransportMode,
) -> Result<String, Diagnostic> {
    let mut methods = Vec::with_capacity(blueprint.routes.len());
    for route in &blueprint.routes {
        let rendered = render_route(codegen, route)?;
        methods.push(Method {
            name: route.handler_name.clone(),
            path_params: rendered.path_params,
            query_params: rendered.query_params,
            request: rendered.request,
            return_ty: rendered.return_ty,
        });
    }

    let mut out = format!(
        "/// The app's internal channel: one method per route, typed.\nmod {} {{\n    use super::*;\n\n",
        reserved::CHANNEL_MODULE
    );
    out.push_str(&render_trait(&methods));
    out.push_str(&render_in_process(&methods));
    if mode == TransportMode::Grpc {
        out.push_str(grpc::GRPC_CLIENT);
        out.push_str(&grpc::render_grpc_impl(&methods));
        out.push_str(&grpc::render_dispatch(&methods));
        out.push_str(grpc::GRPC_SERVER);
    }
    let mut rendered = out.trim_end().to_string();
    rendered.push_str("\n}\n");
    Ok(rendered)
}

/// The trait: the app's route table as typed async methods.
fn render_trait(methods: &[Method]) -> String {
    let mut out =
        String::from("    /// Every route of this app, typed.\n    pub trait Channel {\n");
    for method in methods {
        out.push_str(&format!(
            "        #[allow(non_snake_case)]\n        fn {name}(&self{params}) -> impl std::future::Future<Output = Result<{ret}, String>> + Send;\n",
            name = method.name,
            params = if method.params().is_empty() {
                String::new()
            } else {
                format!(", {}", method.params())
            },
            ret = method.channel_return(),
        ));
    }
    out.push_str("    }\n\n");
    out
}

/// The monolith transport: a direct call into `service`.
fn render_in_process(methods: &[Method]) -> String {
    let mut out = String::from(
        "    /// The monolith transport: a direct call into `service`.\n    ///\n    /// Unused when the project selects the remote transport, so it takes an\n    /// allow rather than a condition.\n    #[derive(Debug, Clone, Copy)]\n    #[allow(dead_code)]\n    pub struct InProcess;\n\n    impl Channel for InProcess {\n",
    );
    for method in methods {
        let body = if method.return_ty.is_empty() {
            format!(
                "            service::{name}({args}).await;\n            Ok(())\n",
                name = method.name,
                args = method.call_args(),
            )
        } else {
            format!(
                "            Ok(service::{name}({args}).await)\n",
                name = method.name,
                args = method.call_args(),
            )
        };
        out.push_str(&format!(
            "        #[allow(non_snake_case)]\n        async fn {name}(&self{params}) -> Result<{ret}, String> {{\n{body}        }}\n",
            name = method.name,
            params = if method.params().is_empty() {
                String::new()
            } else {
                format!(", {}", method.params())
            },
            ret = method.channel_return(),
        ));
    }
    out.push_str("    }\n\n");
    out
}
mod grpc;
