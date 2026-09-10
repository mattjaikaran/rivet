//! The internal channel: one typed method per route, over two transports.
//!
//! The generated app reaches its own service layer through `Channel`. The
//! configured topology decides which implementation the compiler sees:
//! `InProcess` monomorphizes to a direct call, and `Grpc` sends the same
//! payload over a gRPC channel that the app also serves. Both are concrete
//! types, so nothing in the request path looks a transport up.

use super::service::render_route;
use super::{Codegen, rust_str};
use crate::config::TransportMode;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::ServiceBlueprint;

/// The gRPC service name; the client path is `/rivet.Channel/<method>`.
const SERVICE_NAME: &str = "rivet.Channel";

/// One route as the channel sees it: the method name plus its shape.
struct Method {
    name: String,
    params: String,
    return_ty: String,
}

impl Method {
    /// The parameter list without a leading separator.
    fn params(&self) -> &str {
        &self.params
    }

    /// The request parameter name, when the route has one.
    fn request_var(&self) -> Option<&str> {
        self.params.split_once(':').map(|(var, _)| var.trim())
    }

    /// The request parameter type, when the route has one.
    fn request_ty(&self) -> Option<&str> {
        self.params.split_once(':').map(|(_, ty)| ty.trim())
    }

    /// The argument list for a call into `service`.
    fn call_args(&self) -> &str {
        self.request_var().unwrap_or_default()
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
            params: rendered.params,
            return_ty: rendered.return_ty,
        });
    }

    let mut out = String::from(
        "/// The app's internal channel: one method per route, typed.\nmod channel {\n    use super::*;\n\n",
    );
    out.push_str(&render_trait(&methods));
    out.push_str(&render_in_process(&methods));
    if mode == TransportMode::Grpc {
        out.push_str(GRPC_CLIENT);
        out.push_str(&render_grpc_impl(&methods));
        out.push_str(&render_dispatch(&methods));
        out.push_str(GRPC_SERVER);
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
            "        fn {name}(&self{params}) -> impl std::future::Future<Output = Result<{ret}, String>> + Send;\n",
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
        "    /// The monolith transport: a direct call into `service`.\n    #[derive(Debug, Clone, Copy)]\n    pub struct InProcess;\n\n    impl Channel for InProcess {\n",
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
            "        async fn {name}(&self{params}) -> Result<{ret}, String> {{\n{body}        }}\n",
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

/// The remote transport's trait implementation.
fn render_grpc_impl(methods: &[Method]) -> String {
    let mut out = String::from("    impl Channel for Grpc {\n");
    for method in methods {
        let payload = match method.request_var() {
            Some(var) => format!(
                "                let payload = serde_json::to_string(&{var}).map_err(|err| format!(\"cannot encode the `{name}` request: {{err}}\"))?;\n",
                name = method.name,
            ),
            None => "                let payload = String::from(\"null\");\n".to_string(),
        };
        let decode = if method.return_ty.is_empty() {
            "                let _ = self.unary(path, payload).await?;\n                Ok(())\n"
                .to_string()
        } else {
            format!(
                "                let text = self.unary(path, payload).await?;\n                serde_json::from_str(&text).map_err(|err| format!(\"cannot decode the `{name}` response: {{err}}\"))\n",
                name = method.name,
            )
        };
        out.push_str(&format!(
            "        async fn {name}(&self{params}) -> Result<{ret}, String> {{\n            let path = {path};\n{payload}{decode}        }}\n",
            name = method.name,
            params = if method.params().is_empty() {
                String::new()
            } else {
                format!(", {}", method.params())
            },
            ret = method.channel_return(),
            path = rust_str(&format!("/{SERVICE_NAME}/{}", method.name)),
        ));
    }
    out.push_str("    }\n\n");
    out
}

/// The server side: dispatch one decoded call to the matching service
/// function.
fn render_dispatch(methods: &[Method]) -> String {
    let mut out = String::from(
        "    /// Dispatch one inbound call to the service layer.\n    async fn dispatch(method: &str, payload: &str) -> Result<String, tonic::Status> {\n        match method {\n",
    );
    for method in methods {
        let decode_request = match (method.request_var(), method.request_ty()) {
            (Some(var), Some(ty)) => format!(
                "                let {var}: {ty} = serde_json::from_str(payload).map_err(|err| tonic::Status::invalid_argument(format!(\"cannot decode the `{name}` request: {{err}}\")))?;\n",
                name = method.name,
            ),
            _ => String::new(),
        };
        let call = if method.return_ty.is_empty() {
            format!(
                "                service::{name}({args}).await;\n",
                name = method.name,
                args = method.call_args(),
            )
        } else {
            format!(
                "                let value = service::{name}({args}).await;\n",
                name = method.name,
                args = method.call_args(),
            )
        };
        let value = if method.return_ty.is_empty() {
            "()"
        } else {
            "value"
        };
        out.push_str(&format!(
            "            {path} => {{\n{decode_request}{call}                serde_json::to_string(&{value}).map_err(|err| tonic::Status::internal(format!(\"cannot encode the `{name}` result: {{err}}\")))\n            }}\n",
            path = rust_str(&method.name),
            name = method.name,
        ));
    }
    out.push_str(
        "            other => Err(tonic::Status::unimplemented(format!(\"no method `{other}` on this service\"))),\n        }\n    }\n\n",
    );
    out
}

/// The gRPC client half, emitted only in `grpc` mode.
const GRPC_CLIENT: &str = r#"    /// The remote transport: the same calls over a gRPC channel.
    #[derive(Debug, Clone)]
    pub struct Grpc {
        channel: tonic::transport::Channel,
    }

    impl Grpc {
        /// Dial a process that serves this blueprint's channel.
        pub async fn connect(endpoint: String) -> Result<Grpc, String> {
            let channel = tonic::transport::Endpoint::from_shared(endpoint)
                .map_err(|err| format!("invalid service endpoint: {err}"))?
                .connect()
                .await
                .map_err(|err| format!("cannot reach the service channel: {err}"))?;
            Ok(Grpc { channel })
        }

        /// One unary call: send `payload`, return the decoded payload.
        async fn unary(&self, path: &'static str, payload: String) -> Result<String, String> {
            let mut grpc = tonic::client::Grpc::new(self.channel.clone());
            grpc.ready()
                .await
                .map_err(|err| format!("service channel is not ready: {err}"))?;
            let path = http::uri::PathAndQuery::from_static(path);
            grpc.unary(tonic::Request::new(payload), path, JsonCodec)
                .await
                .map(|response| response.into_inner())
                .map_err(|status| format!("service call failed: {status}"))
        }
    }

"#;

/// The gRPC server half: the JSON codec, the service, and the accept loop.
///
/// Rivet writes this by hand instead of running `tonic-build`, because a
/// generated app must build on a machine with only rustc and cargo: no
/// `protoc` and no `prost`. The message is one JSON string, so the schema
/// lives in the DSL rather than in a `.proto` file.
const GRPC_SERVER: &str = r#"    /// A gRPC codec whose message is one JSON string.
    #[derive(Debug, Default, Clone, Copy)]
    struct JsonCodec;

    impl tonic::codec::Codec for JsonCodec {
        type Encode = String;
        type Decode = String;
        type Encoder = JsonEncoder;
        type Decoder = JsonDecoder;

        fn encoder(&mut self) -> JsonEncoder {
            JsonEncoder
        }

        fn decoder(&mut self) -> JsonDecoder {
            JsonDecoder
        }
    }

    #[derive(Debug, Default)]
    struct JsonEncoder;

    impl tonic::codec::Encoder for JsonEncoder {
        type Item = String;
        type Error = tonic::Status;
        fn encode(
            &mut self,
            item: String,
            dst: &mut tonic::codec::EncodeBuf<'_>,
        ) -> Result<(), tonic::Status> {
            use bytes::BufMut;
            dst.put_slice(item.as_bytes());
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct JsonDecoder;

    impl tonic::codec::Decoder for JsonDecoder {
        type Item = String;
        type Error = tonic::Status;

        fn decode(
            &mut self,
            src: &mut tonic::codec::DecodeBuf<'_>,
        ) -> Result<Option<String>, tonic::Status> {
            use bytes::Buf;
            let len = src.remaining();
            if len == 0 {
                return Ok(None);
            }
            let bytes = src.copy_to_bytes(len);
            String::from_utf8(bytes.to_vec()).map(Some).map_err(|err| {
                tonic::Status::internal(format!("the channel payload is not valid UTF-8: {err}"))
            })
        }
    }

    /// The server half of the channel.
    #[derive(Debug, Default, Clone)]
    pub struct Server;

    impl tonic::server::NamedService for Server {
        const NAME: &'static str = "rivet.Channel";
    }

    type BoxFuture<T, E> =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send + 'static>>;

    impl<B> tower::Service<http::Request<B>> for Server
    where
        B: http_body::Body + Send + 'static,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let method = req
                .uri()
                .path()
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string();
            let fut = async move {
                let mut grpc = tonic::server::Grpc::new(JsonCodec);
                Ok(grpc.unary(Dispatch { method }, req).await)
            };
            Box::pin(fut)
        }
    }

    /// One inbound call: the method name plus the decoded payload.
    struct Dispatch {
        method: String,
    }

    impl tonic::server::UnaryService<String> for Dispatch {
        type Response = String;
        type Future = BoxFuture<tonic::Response<String>, tonic::Status>;

        fn call(&mut self, request: tonic::Request<String>) -> Self::Future {
            let method = std::mem::take(&mut self.method);
            let payload = request.into_inner();
            Box::pin(async move { dispatch(&method, &payload).await.map(tonic::Response::new) })
        }
    }

    /// Serve the channel on `listener` until the process ends.
    pub async fn serve(listener: tokio::net::TcpListener) -> Result<(), String> {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tonic::transport::Server::builder()
            .add_service(Server)
            .serve_with_incoming(incoming)
            .await
            .map_err(|err| format!("service channel failed: {err}"))
    }
"#;
