# syntax=docker/dockerfile:1.7
# Clipdeck: Rust server + sample generator, Rust→WebAssembly audio engine, React UI.

# ---------------------------------------------------------------- Rust
FROM rust:1.97-bookworm AS rust
WORKDIR /src
RUN rustup target add wasm32-unknown-unknown
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p clipdeck-server -p clipdeck-samplegen \
 && cargo build --locked -p clipdeck-engine-wasm --lib --target wasm32-unknown-unknown --profile wasm \
 && mkdir -p /out \
 && cp target/release/clipdeck-server target/release/clipdeck-samplegen /out/ \
 && cp target/wasm32-unknown-unknown/wasm/clipdeck_engine_wasm.wasm /out/engine.wasm

# ---------------------------------------------------------------- Web
FROM node:22-bookworm-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web ./
COPY --from=rust /out/engine.wasm public/engine.wasm
RUN npm run build

# ---------------------------------------------------------------- Runtime
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --uid 10001 --create-home clipdeck \
 && mkdir -p /data && chown 10001:10001 /data
COPY --from=rust /out/clipdeck-server /out/clipdeck-samplegen /usr/local/bin/
COPY --from=web /web/dist /app/web
ENV CLIPDECK_DATA=/data \
    CLIPDECK_WEB=/app/web \
    PORT=8080 \
    RUST_LOG=info
USER 10001
EXPOSE 8080
VOLUME /data
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["clipdeck-server"]
