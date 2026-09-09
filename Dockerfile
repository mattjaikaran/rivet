# Multi-stage build for the Rivet CLI.

# Stage 1: builder
FROM rust:1.94-alpine AS builder

# Toolchain and headers for compiling C dependencies (tree-sitter).
RUN apk add --no-cache build-base pkgconfig ca-certificates

WORKDIR /workspace

# Cache dependency compilation: manifests first, then a stub binary, then the
# real sources.
COPY Cargo.toml Cargo.lock ./
COPY rivet-core/Cargo.toml rivet-core/
COPY rivet-cli/Cargo.toml rivet-cli/
RUN mkdir -p rivet-core/src rivet-cli/src \
    && touch rivet-core/src/lib.rs \
    && printf 'fn main() {}\n' > rivet-cli/src/main.rs \
    && cargo build --release --bin rivet \
    && rm -rf rivet-core/src rivet-cli/src

COPY rivet-core/src rivet-core/src
COPY rivet-cli/src rivet-cli/src
RUN cargo build --release --bin rivet

# Stage 2: runtime
FROM alpine:3.21

RUN apk add --no-cache ca-certificates

COPY --from=builder /workspace/target/release/rivet /usr/local/bin/rivet

WORKDIR /app
ENTRYPOINT ["rivet"]
