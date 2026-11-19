# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VulnFeed (vulnfeed) collects high-value vulnerabilities and vendor security-patch notices from multiple sources, stores them, and pushes new items to configured IM bot channels (DingTalk/WeCom/Feishu webhook bots). On top of that sits a Feishu 影响确认/回执 (receipt) subsystem: vulns/notices can spawn confirmation cases, cards are sent to Feishu group chats via an enterprise app, and group members submit impact responses back through a card callback. Rust backend + embedded React SPA. UI text, log messages, and comments are largely in Chinese.

## Commands

### Backend (Rust, edition 2024)

```bash
cargo check --all
cargo clippy --all-targets --all-features --tests --benches -- -D warnings   # CI gate
cargo fmt                                                                     # rustfmt.toml options are nightly-only; stable warns and ignores them
cargo test                                                                    # or: cargo nextest run --all-features
cargo test output::plugins::vuln::kev                                         # run a single test module
cargo run --bin vulnfeed server                                   # start server (config file optional; built-in defaults = dev/config.toml; migrations run automatically)
cargo run --bin vulnfeed change-super-user-password -c ./dev/config.toml -p "新密码"  # change the admin password
```

CLI subcommands are only `server` and `change-super-user-password` (README mentions `create-super-user`; it no longer exists).

No external database is needed: storage is SQLite (`database.path` in the config, default `data/vulnfeed.db`). Migrations in `migrations/` run automatically on `server` and `change-super-user-password` startup. On first boot with an empty `admin_user` table, `server` seeds a default super user `admin`/`admin` — change it in production via `change-super-user-password`.

### Feishu bridge (Go sidecar)

```bash
make feishu-bridge   # builds integrations/feishu-probe into target/feishu-bridge
```

The server auto-starts and supervises `target/feishu-bridge` (`src/output/bridge_supervisor.rs`), auto-generating a 0600 `.feishu-bridge-token` next to the SQLite file on first start. Setting the `VULNFEED_FEISHU_BRIDGE_TOKEN` env var (≥32 chars) switches to external management: no local process is spawned, and the token authenticates the bridge↔server internal API (used by `docker compose --profile feishu`). `target/` cleanup requires re-running `make feishu-bridge`.

### Frontend (`assets/`, React + Vite + Tailwind, managed with bun)

```bash
cd assets
bun install
bun dev        # dev server on :3000, proxies /api to localhost:9000
bun run build  # outputs to assets/public/
```

`bun.lock` is the lockfile (`bun install --frozen-lockfile` in CI/Docker). Do not reintroduce pnpm/npm lockfiles.

### Static asset embedding

`cargo build` embeds the frontend into the binary: `build.rs` builds the SPA with bun when `assets/public` is missing (skipped when bun is absent or `VULNFEED_SKIP_FRONTEND_BUILD=1`; the build still succeeds with no frontend), stages it into `public/` (gitignored except `.gitkeep`), and emits `VULNFEED_EMBEDDED_ASSETS_HASH` so asset changes recompile the crate. rust-embed has `debug-embed` enabled — assets are embedded in debug builds too. Alternatively set `server.static_dir` (or `VULNFEED_CONFIG_SERVER_STATIC_DIR`) to serve the SPA from a directory on disk; served by `SpaDirEndpoint` with an index.html fallback.

### Make / Docker

```bash
make docker          # default goal: zigbuild current ARCH locally, copy binary into debian:bookworm-slim (--load)
make ARCH=amd64|arm64 docker   # pick a target triple
make docker-multi    # buildx build linux/amd64+arm64 and push IMAGE_REPO:IMAGE_TAG
make feishu-bridge   # build the Go Feishu bridge sidecar
```

docker-compose runs `vulnfeed` plus an optional `feishu` profile: `docker compose --profile feishu up -d --build` starts the bridge container sharing the vulnfeed network namespace (loopback internal API), both configured with the same `VULNFEED_FEISHU_BRIDGE_TOKEN`. SQLite data persists via the `./data/` volume mount.

## Architecture

Hexagonal-style layout with the flow: **scheduler → plugins → mpsc channel → worker → DB + push channels**, plus the parallel **receipt pipeline: worker hook → receipt_case/task → background delivery → Feishu card → bridge callback → response → card refresh**.

- `src/domain/` — models and ports. `ports.rs` defines `VulnService`/`VulnRepository` traits; `services.rs` is a thin pass-through impl.
- `src/input/http/` — poem web server. `/api/*` routes behind JWT `AuthMiddleware`; everything else falls through to the SPA endpoint (`spa_endpoint.rs`) which serves files embedded from `public/` via rust-embed. **Exception:** `/api/internal/feishu/*` (`card-action`, `apps`) is authenticated by the bridge token (HMAC-SHA256 bearer), not JWT, and is disabled (503) when no token is configured.
- `src/output/` — infrastructure:
  - `db/` — **Sea ORM** on top of the sqlx SQLite pool (WAL). `Db` wraps a `sea_orm::DatabaseConnection` (use `db.orm()`); migrations still run via the **sqlx** migrator to retain existing history/checksums on deployed databases. `base.rs` holds generic DAO helpers (`dao_create`, `dao_upsert`, `DaoQueryBuilder`, …) built on sea-orm transactions + sea-query; per-table DAOs are thin wrappers. Newer tables use sea-orm entities in `db/entities/` (`feishu_app`, `receipt_delivery`, `receipt_team`). `repository_impl.rs` implements `VulnRepository` for the pool wrapper `Db`.
  - `plugins/vuln/` and `plugins/sec_notice/` — the two scraper families. Each plugin implements `VulnPlugin`/`SecNoticePlugin`, fetches/parses its source, and sends `Create*` records over its unbounded mpsc sender. `sync_task` rows control per-source scheduling, `skip_tls_verify`, and the split auto-receipt switches: `auto_receipts` (vulns) and `auto_receipts_notice` (notices, default off — notices are already filtered by stack subscriptions).
  - `worker.rs` / `sec_notcie_worker.rs` — consume the channels: dedupe by `key` and merge same-CVE records across sources (first source wins, worst severity kept, pushed once), enrich new CVEs with GitHub/nuclei POC search (`utils/search.rs`), upsert, push new items to all enabled `push_channel` rows (markdown rendered with Tera templates in `push/`), and call `receipts::auto_create` for sources with `auto_receipts` enabled.
  - `scheduler.rs` — tokio-cron-scheduler job (interval from the `sync_task` DB row) that runs all plugins' `update()` concurrently via JoinSet.
  - `push/` — `MessageBot` trait with ding_bot/feishu_bot(wecom-style webhook)/wecom_bot impls in a DashMap registry (driven by `push_channel` rows). `feishu_app.rs` is NOT a MessageBot: it's the app-credential Feishu API client (tenant token, card send/patch/reply, per-user contact lookup) used by the receipt subsystem.
  - Receipt subsystem (dimension model, SOP v24 安全漏洞处理流程):
    - `receipts.rs` — core state machine: cases (`receipt_case`), one task per case (`receipt_task`), dimension answers (`receipt_response`), audit events. Three confirmation dimensions live in `receipt_dimension` (P 产线/NovoOne 产线/运维, seeded with owner names; owner open_ids configured via `PUT /api/receipts/dimensions/:id`). A card submission = dimension + one of five options (`none/ops/iter/both/special`, labels in `RECEIPT_STATUSES`) + 处置负责人 owners (select_person; `none`→0, `ops/iter`→exactly 1, `both/special`→1..5). One latest answer per dimension (回执确认只需一次); owners persist as JSON `[{"open_id","name"}]`. `auto_create` gates on `sync_task.auto_receipts` + a rolling freshness window (disclosure ≥ local yesterday); close is blocked until every enabled dimension answered (`Incomplete`). Legacy three-state rows (`investigating/unaffected/affected`, `dimension=''`) stay readable everywhere but don't complete a dimension.
    - `feishu_receipts.rs` — builds and sends interactive cards (dimension/status/`select_person` owners form; per-dimension ✅/⏳ survey summary + detail table; markdown fallback via `strip_table`), dimension-based `reminder_card` nudge, handles card callbacks (`CardCallback`; a rejected action still enqueues a card refresh so old-form cards get replaced).
    - `receipt_auto_delivery.rs` — background loop delivering queued cases with retry/backoff (spawned in `cli.rs`).
    - `receipt_refresh.rs` — background loop refreshing delivered cards after responses (spawned in `cli.rs`).
    - `receipt_name_lookup.rs` — background loop (every 30s, also triggered immediately after each card callback) resolving each answer's operator open_id → display name + first department, and empty owner names inside the owners JSON, via the per-user contact API (`department_id_type=department_id`); results persist on `receipt_response` (name in `display_name`, department in `team`, owner names in `owners`), failures retry every 5 min. Requires the app's contact **data scope** (通讯录权限范围 = all members) to be effective, not just the API scopes.
    - `receipt_remind.rs` — dimension-based reminders: every 24h while a dimension is pending, no count cap but only for cases opened within the last 7 days (`REMIND_MAX_AGE_SECS`); from the 3rd reminder on also @ `sync_task.remind_escalate_open_id` (升级提醒人).
    - `receipt_member_sync.rs` — retired loop (kept for reference): cached chat member lists for response-rate display. Not spawned anymore; the manual `POST /api/receipts/directory/sync-members` endpoint still exists. Requires `im:chat.members:read` which this app never had.
    - `bridge_supervisor.rs` — local Go bridge process lifecycle (see "Feishu bridge" above).
    - HTTP handlers: `receipts.rs` (list/detail/export CSV/remind; `receipt_summaries` also feeds receipt badges into the vuln and notice list DTOs), `feishu_apps.rs`, `feishu_directory.rs`, `feishu_bridge.rs` (the internal card-action endpoint).
- `src/config/settings.rs` — TOML config (`dev/config.toml`) with `VULNFEED_CONFIG_*` env overrides; `deny_unknown_fields`, unknown env vars are an error.
- `docs/` — Feishu receipt design docs. **The current model is the dimension-based SOP v24 (五选一 + 处置负责人, described in `CLAUDE.md` here and implemented in `receipts.rs`)**; `feishu-individual-receipts.md` documents the superseded individual model (kept for history — legacy rows still render), `feishu-vulnerability-receipts.md` is the original design draft; `feishu-identity-and-group-model.md` is superseded. When behavior and docs disagree, the code wins.

### Plugin registry pattern

Plugins self-register in a global `DashMap` registry from their `try_new()`, and each family's `init()` (called from `src/cli.rs` at startup) constructs every plugin. Adding a data source = new module under the family + one `try_new` line in that family's `init()` + `pub mod` entry.

### SQLite specifics

- List columns (`tags`, `reference_links`, `github_search`, `reasons`) are `TEXT` storing JSON; the model type is `SqlJsonVecString` (`src/domain/models/sql_json_vec.rs`) which converts at the sea-query/sqlx boundary — construct with `.into()` from `Vec<String>`.
- `dao_upsert` uses `INSERT … ON CONFLICT DO NOTHING RETURNING`; on conflict SQLite returns no row, so the fallback re-selects the id by the conflict column.
- `updated_at`/`created_at` are TEXT `CURRENT_TIMESTAMP` defaults, decoded into `DateTime<Utc>` by sqlx.
- App secrets are stored in `feishu_app` rows; they are write-only via the admin API (never returned in plaintext) and `FeishuApp` deliberately does not derive `Debug` so secrets can't leak into logs.

### Testing patterns

- DB-touching unit tests spin up a temp-file SQLite via `Db::new` with `config.database.path` pointed at a temp path, run `db.migrate()`, and seed with raw SQL `Statement`s — see the tests at the bottom of `receipt_auto_delivery.rs` / `receipt_refresh.rs` / `worker.rs`.
- Plugin unit tests hit the **live external sites** — they require network access and are inherently flaky.
- `bridge_supervisor.rs` tests spawn a real shell-script "bridge" binary.

## Repo conventions

- Pre-commit hooks (`.pre-commit-config.yaml`) and CI (`.github/workflows/ci.yml`) run: `cargo fmt --check`, `cargo check --all`, clippy `-D warnings`, `cargo nextest run --all-features`.
- Import style follows rustfmt.toml: grouped std/external/crate, item granularity (nightly rustfmt fully applies it).
- Commit messages follow `feat: ...` conventional style (see git log).
