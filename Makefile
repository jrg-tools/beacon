# Local development. `make help` lists the targets.
# Settings come from .env (see .env.example); make exports them to the server, so any shell works.

-include .env
export

WEB_SRC := $(shell find web/src web/static -type f) web/vite.config.ts web/package.json

.PHONY: help env run dev test fmt docker clean

help: ## List the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-8s %s\n", $$1, $$2}'

# Not a `.env:` rule: make would try to remake the included file on every run.
env:
	@test -f .env || { cp .env.example .env; echo "Created .env: fill in the Clerk keys, then run make again."; exit 1; }

web/node_modules: web/package.json web/pnpm-lock.yaml
	cd web && pnpm install --frozen-lockfile
	@touch $@

# Rebuilt only when the UI's sources change.
web/build/index.html: web/node_modules $(WEB_SRC)
	cd web && pnpm run build

run: env web/build/index.html ## Build the UI if needed and start the server on $BIND
	cargo run

dev: env web/node_modules ## Server plus the UI with hot reload (open the Vite URL + /admin)
	@trap 'kill 0' INT TERM; cargo run & (cd web && pnpm dev) & wait

test: web/node_modules ## Every check CI runs
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	cargo test --locked
	cd web && pnpm run lint && pnpm run check && pnpm run test && pnpm audit && pnpm run build

fmt: web/node_modules ## Format Rust and the UI
	cargo fmt
	cd web && pnpm run format

docker: ## Build the image and run it with .env (data in the `beacon` volume)
	docker build -t beacon .
	docker run --rm -p 8080:8080 -v beacon:/data --env-file .env beacon

clean: ## Remove build output (keeps ./data)
	cargo clean
	rm -rf web/build web/.svelte-kit
