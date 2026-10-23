# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VulnFeed (vulnfeed) collects high-value vulnerabilities and vendor security-patch notices from multiple sources, stores them, and pushes new items to a DingTalk (钉钉) bot. Rust backend + embedded React SPA. UI text, log messages, and comments are largely in Chinese.

## Commands

### Backend (Rust, edition 2024)

```bash
cargo check --all
cargo clippy --all-targets --all-features --tests --benches -- -D warnings   # CI gate
cargo fmt                                                                     # rustfmt.toml options are nightly-only; stable warns and ignores them
cargo test                                                                    # or: cargo nextest run --all-features
cargo test output::plugins::vuln::kev                                         # run a single test module
cargo run --bin vulnfeed server -c ./dev/config.toml                          # start server (runs migrations automatically)
cargo run --bin vulnfeed create-super-user -c ./dev/config.toml -p "密码"     # create/reset the admin user
```

No external database is needed: storage is SQLite (`database.path` in the config, default `data/vulnfeed.db`). Migrations in `migrations/` run automatically on both `server` and `create-super-user` startup.

### Frontend (`assets/`, React + Vite + Tailwind, managed with bun)

```bash
cd assets
bun install
bun dev        # dev server on :3000, proxies /api to localhost:9000
bun run build  # outputs to assets/public/
```

`bun.lock` is the lockfile (`bun install --frozen-lockfile` in CI/Docker). Do not reintroduce pnpm/npm lockfiles.

### Static asset embedding

`cargo build` embeds the frontend into the binary: `build.rs` builds the SPA with bun when `assets/public` is missing (skipped when bun is absent or `VULNFEED_SKIP_FRONTEND_BUILD=1`; the build still succeeds with no frontend), stages it into `public/` (gitignored except `.gitkeep`), and emits `VULNFEED_EMBEDDED_ASSETS_HASH` so asset changes recompile the crate. rust-embed has `debug-embed` enabled — assets are embedded in debug builds too.

### Docker

```bash
docker build -t vulnfeed:latest .   # builds frontend with bun, embeds it into the binary image
```

docker-compose runs only the `vulnfeed` service; SQLite data persists via the `./data/` volume mount.

## Architecture

Hexagonal-style layout with the flow: **scheduler → plugins → mpsc channel → worker → DB + DingTalk push**.

- `src/domain/` — models and ports. `ports.rs` defines `VulnService`/`VulnRepository` traits; `services.rs` is a thin pass-through impl.
- `src/input/http/` — poem web server. `/api/*` routes behind JWT `AuthMiddleware`; everything else falls through to the SPA endpoint (`spa_endpoint.rs`) which serves files embedded from `public/` via rust-embed (`build.rs` stages the frontend build output there — see "Static asset embedding" below).
- `src/output/` — infrastructure:
  - `db/` — sqlx (SQLite pool, WAL) + sea-query. `base.rs` holds the generic DAO helpers (`dao_create`, `dao_upsert`, `DaoQueryBuilder`, …); per-table DAOs are thin wrappers. `repository_impl.rs` implements `VulnRepository` for the pool wrapper `Db`.
  - `plugins/vuln/` and `plugins/sec_notice/` — the two scraper families. Each plugin implements `VulnPlugin`/`SecNoticePlugin`, fetches/parses its source, and sends `Create*` records over its unbounded mpsc sender.
  - `worker.rs` / `sec_notcie_worker.rs` — consume the channels: dedupe by `key`, enrich with GitHub/nuclei POC search (only for new CVEs), upsert, and push new items to DingTalk (markdown rendered with Tera templates in `push/`).
  - `scheduler.rs` — tokio-cron-scheduler job (interval from the `sync_task` DB row) that runs all plugins' `update()` concurrently via JoinSet.
- `src/config/settings.rs` — TOML config (`dev/config.toml`) with `VULNFEED_CONFIG_*` env overrides; `deny_unknown_fields`, unknown env vars are an error.

### Plugin registry pattern

Plugins self-register in a global `DashMap` registry from their `try_new()`, and each family's `init()` (called from `src/cli.rs` at startup) constructs every plugin. Adding a data source = new module under the family + one `try_new` line in that family's `init()` + `pub mod` entry. Plugin unit tests hit the **live external sites** — they require network access and are inherently flaky.

### SQLite specifics

- List columns (`tags`, `reference_links`, `github_search`, `reasons`) are `TEXT` storing JSON; the model type is `SqlJsonVecString` (`src/domain/models/sql_json_vec.rs`) which converts at the sea-query/sqlx boundary — construct with `.into()` from `Vec<String>`.
- `dao_upsert` uses `INSERT … ON CONFLICT DO NOTHING RETURNING`; on conflict SQLite returns no row, so the fallback re-selects the id by the conflict column.
- `updated_at`/`created_at` are TEXT `CURRENT_TIMESTAMP` defaults, decoded into `DateTime<Utc>` by sqlx.

## Repo conventions

- Pre-commit hooks (`.pre-commit-config.yaml`) and CI (`.github/workflows/ci.yml`) run: `cargo fmt --check`, `cargo check --all`, clippy `-D warnings`, `cargo nextest run --all-features`.
- Import style follows rustfmt.toml: grouped std/external/crate, item granularity (nightly rustfmt fully applies it).
- Commit messages follow `feat: ...` conventional style (see git log).
