//! The gRPC transport: the trait implementation, the client payload, and the
//! server-side dispatch.
//!
//! Separated from [`super`] so the monolith path and the wire path stay two
//! readable concerns. The payload is the argument list: one argument travels
//! as itself, several as a tuple, so a path parameter and a body share one
//! message.

use super::super::rust_str;
use super::{Method, SERVICE_NAME};

/// The remote transport's trait implementation.
pub(super) fn render_grpc_impl(methods: &[Method]) -> String {
    let mut out = String::from("    impl Channel for Grpc {\n");
    for method in methods {
        let args = method.arguments();
        let payload = match args.len() {
            0 => "                let payload = String::from(\"null\");\n".to_string(),
            1 => {
                let (var, _) = &args[0];
                format!(
                    "                let payload = serde_json::to_string(&{var}).map_err(|err| format!(\"cannot encode the `{name}` request: {{err}}\"))?;\n",
                    name = method.name,
                )
            }
            _ => {
                let names: Vec<&str> = args.iter().map(|(name, _)| name.as_str()).collect();
                format!(
                    "                let payload = serde_json::to_string(&({})).map_err(|err| format!(\"cannot encode the `{name}` request: {{err}}\"))?;\n",
                    names.join(", "),
                    name = method.name,
                )
            }
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
            "        #[allow(non_snake_case)]\n        async fn {name}(&self{params}) -> Result<{ret}, String> {{\n            let path = {path};\n{payload}{decode}        }}\n",
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
pub(super) fn render_dispatch(methods: &[Method]) -> String {
    let mut out = String::from(
        "    /// Dispatch one inbound call to the service layer.\n    async fn dispatch(method: &str, payload: &str) -> Result<String, tonic::Status> {\n        match method {\n",
    );
    for method in methods {
        let args = method.arguments();
        let decode_request = match args.len() {
            0 => String::new(),
            1 => {
                let (var, ty) = &args[0];
                format!(
                    "                let {var}: {ty} = serde_json::from_str(payload).map_err(|err| tonic::Status::invalid_argument(format!(\"cannot decode the `{name}` request: {{err}}\")))?;\n",
                    name = method.name,
                )
            }
            _ => {
                let names: Vec<&str> = args.iter().map(|(name, _)| name.as_str()).collect();
                let types: Vec<&str> = args.iter().map(|(_, ty)| ty.as_str()).collect();
                format!(
                    "                let ({names}): ({types}) = serde_json::from_str(payload).map_err(|err| tonic::Status::invalid_argument(format!(\"cannot decode the `{name}` request: {{err}}\")))?;\n",
                    names = names.join(", "),
                    types = types.join(", "),
                    name = method.name,
                )
            }
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
pub(super) const GRPC_CLIENT: &str = r#"    /// The remote transport: the same calls over a gRPC channel.
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
pub(super) const GRPC_SERVER: &str = r#"    /// A gRPC codec whose message is one JSON string.
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
