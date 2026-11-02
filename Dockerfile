# syntax=docker/dockerfile:1

# The admin UI: a static SvelteKit build the Rust server serves (admin pages under /admin, the install page at /firmware).
# Pinned by digest; bump the tag and digest together.
FROM node:26-alpine@sha256:c6ad9e9c1d80f6eb607c26b4b0a132ff7a2344fad1c78ea3baa2a7d4c8de6d94 AS web
WORKDIR /web
# Node 25+ images no longer ship corepack: install it, then it reads pnpm's version from `packageManager`.
RUN npm install -g corepack@latest && corepack enable
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml web/.npmrc ./
RUN --mount=type=cache,id=pnpm-store,target=/pnpm/store \
	pnpm install --frozen-lockfile --store-dir /pnpm/store
COPY web ./
RUN pnpm run build

# A static musl binary (SQLite and TLS compiled in).
FROM rust:1-alpine@sha256:7e9e2d068a107a2adafd19930011178d14a78326053cd4c3a9f7ad3fd049557d AS server
RUN apk add --no-cache build-base cmake perl ca-certificates
WORKDIR /src
# Dependencies get their own layer (a stub main), so the CI layer cache keeps them until Cargo.lock changes.
# No cache mounts here: CI never saves them, and the layer would then hold nothing.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
	&& cargo build --release --locked \
	&& rm -rf src
COPY migrations ./migrations
COPY src ./src
# touch: the checked-out sources can be older than the stub build, and cargo would skip them.
RUN touch src/main.rs \
	&& cargo build --release --locked \
	&& cp target/release/beacon /beacon \
	&& mkdir -p /data/cache

# Nothing but the binary, the UI and the CA roots.
FROM scratch
COPY --from=server /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=server /beacon /beacon
COPY --from=web /web/build /ui
# The database and the firmware cache are never in the image: they live on this volume.
COPY --from=server --chown=65532:65532 /data /data
ENV DATA_DIR=/data UI_DIR=/ui BIND=0.0.0.0:8080 RUST_LOG=info
VOLUME ["/data"]
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/beacon"]
