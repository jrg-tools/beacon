# syntax=docker/dockerfile:1

# The admin UI: a static SvelteKit build the Rust server serves (admin pages under /admin, the install page at /firmware).
FROM node:26-alpine AS web
WORKDIR /web
# Node 25+ images no longer ship corepack: install it, then it reads pnpm's version from `packageManager`.
RUN npm install -g corepack@latest && corepack enable
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml web/.npmrc ./
RUN --mount=type=cache,id=pnpm-store,target=/pnpm/store \
	pnpm install --frozen-lockfile --store-dir /pnpm/store
COPY web ./
RUN pnpm run build

# A static musl binary (SQLite and TLS compiled in).
FROM rust:1-alpine AS server
RUN apk add --no-cache build-base cmake perl ca-certificates
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
	--mount=type=cache,target=/src/target \
	cargo build --release --locked \
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
