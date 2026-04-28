# syntax=docker/dockerfile:1.7
#
# Multi-stage Dockerfile for kronroe-docs-api.
#
# Stage 1 (builder): full Rust toolchain + system deps to compile the
# binary + bundle the fastembed ONNX model.
#
# Stage 2 (runtime): distroless base image with just the binary, model,
# and libstdc++ (needed by the ONNX runtime). Final image stays under
# ~150MB despite shipping the embedding model inline.
#
# Built and pushed by the deploy workflow (Phase 3b). For local
# development, run with:
#
#   docker build -t kronroe-docs-api -f crates/docs-api/Dockerfile .
#   docker run --rm -p 8080:8080 kronroe-docs-api

# ─── Stage 1: builder ─────────────────────────────────────────────
FROM rust:1.83-slim-bookworm AS builder

# fastembed pulls ONNX runtime via the `ort` crate which needs cmake
# at build time and libstdc++/libgomp at runtime. Keeping these in the
# builder layer only.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        cmake \
        pkg-config \
        libssl-dev \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy the workspace manifest and crate manifests first so dependency
# resolution / compilation can be cached separately from source edits.
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml crates/core/Cargo.toml
COPY crates/agent-memory/Cargo.toml crates/agent-memory/Cargo.toml
COPY crates/ios/Cargo.toml crates/ios/Cargo.toml
COPY crates/android/Cargo.toml crates/android/Cargo.toml
COPY crates/mcp-server/Cargo.toml crates/mcp-server/Cargo.toml
COPY crates/wasm/Cargo.toml crates/wasm/Cargo.toml
COPY crates/python/Cargo.toml crates/python/Cargo.toml
COPY crates/docs-api/Cargo.toml crates/docs-api/Cargo.toml

# Now copy source and build. Only the docs-api binary is needed.
COPY crates ./crates

RUN cargo build --release --package kronroe-docs-api --locked

# ─── Stage 2: runtime ────────────────────────────────────────────
# distroless ships glibc but not libstdc++. Use the cc variant which
# includes libstdc++/libgomp needed by ORT.
FROM gcr.io/distroless/cc-debian12

WORKDIR /app

COPY --from=builder /build/target/release/kronroe-docs-api /app/kronroe-docs-api

# Cloud Run injects PORT; default 8080 for local dev.
ENV PORT=8080
EXPOSE 8080

# fastembed downloads its model on first init. For Cloud Run we'll
# want to pre-warm this in the image (Phase 3b adds a build step that
# `embed("warmup")` once during build to materialise the model file).
# For the spike we accept a slow first request.

ENTRYPOINT ["/app/kronroe-docs-api"]
