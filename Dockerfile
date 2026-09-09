# Multi-stage build for Rivet CLI and the generated services

# Stage 1: Builder
FROM rust:1.81-alpine AS builder

# Install dependencies for compiling Rust and PostgreSQL development headers
RUN apk add --no-cache \
    musl-dev \
    pkgconfig \
    openssl-dev \
    libpq-dev \
    build-base \
    git \
    ca-certificates

WORKDIR /workspace

# Cache dependencies (Cargo.toml + Cargo.lock)
COPY Cargo.toml Cargo.lock ./
COPY rivet-core/Cargo.toml rivet-core/
COPY rivet-cli/Cargo.toml rivet-cli/

# Create a dummy main.rs to cache dependency compilation
RUN mkdir -p rivet-core/src && echo "fn main() {}" > rivet-core/src/lib.rs
RUN mkdir -p rivet-cli/src && echo "fn main() { println!(\"Hello\"); }" > rivet-cli/src/main.rs
RUN cargo build --release --bin rivet
RUN rm -rf rivet-core/src rivet-cli/src

# Copy actual source code
COPY rivet-core/src ./rivet-core/src
COPY rivet-cli/src ./rivet-cli/src

# Build the actual binary
RUN cargo build --release --bin rivet

# Stage 2: Runner
FROM alpine:3.20 AS runner

# Install runtime dependencies (PostgreSQL client, SSL, and CA certificates)
RUN apk add --no-cache \
    ca-certificates \
    openssl \
    libpq \
    curl

WORKDIR /app

# Copy the built binary
COPY --from=builder /workspace/target/release/rivet /usr/local/bin/rivet

# Copy entrypoint
COPY docker-entrypoint.sh /usr/local/bin/entrypoint
RUN chmod +x /usr/local/bin/entrypoint

# Default command
ENTRYPOINT ["/usr/local/bin/entrypoint"]

EXPOSE 3000