# ---------- Stage 1: build the React frontend ----------
FROM node:20-alpine AS web-build
WORKDIR /web
# Install deps first so this layer caches when only source files change.
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# ---------- Stage 2: build the Rust server ----------
FROM rust:1-slim-bookworm AS server-build
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
# Copy the workspace manifests + lockfile first so Cargo's dep build can
# cache across source-only changes. We also copy the per-crate manifests
# but not their src dirs yet — a dummy src placeholder fools Cargo into
# fetching + compiling deps before we copy the real code.
COPY Cargo.toml Cargo.lock ./
COPY crates/analyzer-core/Cargo.toml  crates/analyzer-core/Cargo.toml
COPY crates/analyzer-cli/Cargo.toml   crates/analyzer-cli/Cargo.toml
COPY crates/analyzer-server/Cargo.toml crates/analyzer-server/Cargo.toml
RUN mkdir -p crates/analyzer-core/src crates/analyzer-cli/src crates/analyzer-server/src \
    && echo 'pub fn _stub() {}' > crates/analyzer-core/src/lib.rs \
    && echo 'fn main() {}'      > crates/analyzer-cli/src/main.rs \
    && echo 'fn main() {}'      > crates/analyzer-server/src/main.rs \
    && cargo build --release -p analyzer-server --locked || true

# Now copy the real source and build for real.
COPY crates ./crates
# include_str! references — these have to exist at compile time.
COPY sample.log ./sample.log
COPY samples ./samples
RUN cargo build --release -p analyzer-server --locked

# ---------- Stage 3: minimal runtime ----------
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server-build /src/target/release/log-analyzer-server /app/log-analyzer-server
COPY --from=web-build    /web/dist /app/web/dist
ENV STATIC_DIR=/app/web/dist
# Render sets $PORT; the server picks it up automatically. Default 8080
# for non-Render hosts.
ENV PORT=8080
EXPOSE 8080
CMD ["/app/log-analyzer-server"]
