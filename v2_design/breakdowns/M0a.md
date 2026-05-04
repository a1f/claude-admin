> **GitHub issue:** [a1f/claude-admin#1](https://github.com/a1f/claude-admin/issues/1)
> **Generated:** 2026-04-28
> **Plan:** [v2_design](file:///Users/alf/dev/claude_admin/v2_design/00-final-plan.html)

# M0a · Server skeleton + auth + deploy

> **Goal.** Web server live on a real VPS at `https://your-server`. GitHub OAuth login works. `/healthz` green. Reproducible deploy. Quota circuit breaker scaffolding stubbed in.

**Phase:** `foundation` · **Kind:** `infra` · **Target PRs:** 5 · **Risk:** low–medium

**Exit criteria.** _kill server, redeploy in <5min, login still works_

**Milestone depends on:** none (this is the first milestone)

**Plan:** [v2_design](file:///Users/alf/dev/claude_admin/v2_design/00-final-plan.html) · canonical doc: `v2_design/00-final-plan.html`

---

## Task checklist

- [ ] **M0a-T1** — Repo scaffold + CI gates _(~80 LOC)_
- [ ] **M0a-T2** — HTTP server skeleton with /healthz + structured logging _(~150 LOC)_
- [ ] **M0a-T3** — GitHub OAuth flow + signed session cookie + single-user lock _(~200 LOC)_
- [ ] **M0a-T4** — Deploy pipeline: Caddy + systemd + Postgres + sqlx-migrate _(~150 LOC)_
- [ ] **M0a-T5** — Quota tracker stub: schema + endpoints _(~150 LOC)_

**Aggregate:** 5 tasks · ~730 LOC

---

## Tasks

### M0a-T1 · Repo scaffold + CI gates

**Deliverable.** Cargo workspace with `server` and `daemon` crates (placeholder mains). Lint/format configs. GitHub Actions CI running fmt, clippy, and test on every PR.

**Expectation.** After this PR: a clean clone runs `cargo build --workspace` green. Pushing a PR triggers CI gates that block merge if any of fmt/clippy/test fails.

**Scope.**
- New: `Cargo.toml` (workspace), `rustfmt.toml`, `clippy.toml`, `.github/workflows/ci.yml`, `crates/server/{Cargo.toml, src/main.rs, src/lib.rs}`, `crates/daemon/{Cargo.toml, src/main.rs}`, `README.md` with "how to build" section
- Out of scope: any real server logic, OAuth, DB, Docker

**Motivation.** Every following task ships through CI. Without this, no signal on compilation / formatting / lint — and we'd have to retrofit gates later. Establishes the "PR mergeable" baseline.

**Validation.**
- `cargo build --workspace` succeeds locally and in CI
- `cargo fmt --check` and `cargo clippy --workspace -- -D warnings` pass
- CI workflow runs fmt + clippy + test as required checks; an intentionally-broken commit on a side branch fails CI

**Test scenarios.**
- _unit_ — **server_lib_smoke**: trivial assertion in `crates/server/src/lib.rs` to verify `cargo test` is wired correctly
- _integration_ — **ci_blocks_lint_failure**: branch with an unused-var clippy warning fails CI; removing the warning makes CI green

**Blockers:** none
**Estimated LOC:** ~80

---

### M0a-T2 · HTTP server skeleton with /healthz + structured logging

**Deliverable.** axum server bound to configurable port. Single route `GET /healthz` returns 200 + JSON `{"status":"ok","version":"<git-sha>","uptime_s":<n>}`. `tracing-subscriber` with JSON output behind `RUST_LOG`. Graceful shutdown on SIGINT / SIGTERM.

**Expectation.** After this PR: `cargo run -p server` starts the server; `curl localhost:8080/healthz` returns 200 with the JSON; structured logs go to stdout; ctrl-C exits within 1s without dropping in-flight connections.

**Scope.**
- Modified: `crates/server/src/main.rs`, `crates/server/src/lib.rs`
- New: `crates/server/src/health.rs` (handler), `crates/server/src/config.rs` (env-driven `Config`), `crates/server/src/observability.rs` (tracing setup)
- Out of scope: auth, DB, OAuth, additional routes

**Motivation.** Foundation for every subsequent route. `/healthz` becomes the smoke test for deploys (M0a-T4 uses it). Logging set up here so all subsequent code logs consistently.

**Validation.**
- `GET /healthz` returns 200 + JSON with `status`, `version`, `uptime_s`
- Server picks up `PORT` env var and falls back to 8080
- `RUST_LOG=info cargo run -p server` produces JSON logs (one per line) to stdout
- SIGTERM causes graceful shutdown; in-flight request completes before exit

**Test scenarios.**
- _unit_ — **health_handler_returns_ok_json**: call handler directly, assert response body shape
- _integration_ — **healthz_e2e**: spawn the server in a test, GET /healthz, assert 200 and parse the JSON body
- _integration_ — **graceful_shutdown_completes_inflight_request**: spawn server, fire a slow mock request, send SIGTERM, assert the request completes before the process exits

**Blockers:** M0a-T1 merged
**Estimated LOC:** ~150

---

### M0a-T3 · GitHub OAuth flow + signed session cookie + single-user lock

**Deliverable.** `/auth/login` redirects to GitHub OAuth. `/auth/callback` handles the code exchange and sets a signed session cookie (via `tower-cookies`). `/me` returns the logged-in user's GH login. Protected routes use middleware that 401s without a valid cookie. `ALLOWED_GH_LOGIN` env var hard-codes a single allowed login.

**Expectation.** After this PR: opening `/auth/login` redirects to GitHub authorization, then back to `/me` showing my GH login. Visiting any protected route without the cookie returns 401. Logging in as a different GH account is rejected with 403.

**Scope.**
- New: `crates/server/src/auth.rs` (handlers + middleware), `crates/server/src/auth/session.rs` (cookie sign/verify with HMAC-SHA256)
- Modified: `crates/server/src/main.rs` (mount routes + middleware), `crates/server/src/config.rs` (add `GH_CLIENT_ID`, `GH_CLIENT_SECRET`, `SESSION_KEY`, `ALLOWED_GH_LOGIN`)
- Out of scope: multi-user, role-based access, refresh tokens, OAuth state-store backed by DB (in-memory + signed nonce is fine here)

**Motivation.** Auth gate before any orchestration endpoints exist. Single-user lock prevents accidental exposure on a public VPS. Cookie auth is required for the React SPA in M2c.

**Validation.**
- `/auth/login` → 302 to `github.com/login/oauth/authorize`
- `/auth/callback` exchanges code for token, sets `HttpOnly` + `SameSite=Lax` + `Secure` cookie, redirects to `/`
- `/me` returns 200 with `{login: "..."}` when cookie is present and login matches `ALLOWED_GH_LOGIN`
- `/me` returns 401 when no cookie
- Login as a different GH account: 403
- Cookie is signed; tampering invalidates it

**Test scenarios.**
- _unit_ — **session_cookie_roundtrip_signs_and_verifies**: encode a session, tamper a byte, decode fails
- _integration_ — **oauth_callback_rejects_disallowed_login**: stub GH API returning a different login, callback returns 403
- _integration_ — **protected_route_401s_without_cookie**: call `/me` without cookie, assert 401
- _e2e_ — **full_login_flow_via_test_oauth_provider**: spawn server with a fake-OAuth-provider env, drive curl through `/auth/login` → `/auth/callback` → `/me`, assert 200 with expected login

**Blockers:** M0a-T2 merged
**Estimated LOC:** ~200

---

### M0a-T4 · Deploy pipeline: Caddy + systemd + Postgres + sqlx-migrate

**Deliverable.** `deploy/` directory with Caddy config (HTTPS via Let's Encrypt), systemd unit for the server binary, idempotent Postgres install steps, `sqlx migrate run` integrated into deploy. Single `make deploy HOST=user@host` command that ships the build, runs migrations, restarts the service, and confirms `/healthz` is green over HTTPS.

**Expectation.** After this PR: `make deploy HOST=alf@my-vps` from a clean checkout takes the running server from "nothing" to "live with /healthz green over HTTPS" in one command. Re-running deploys a new build; migrations run automatically; systemd restart drains in-flight.

**Scope.**
- New: `deploy/Caddyfile`, `deploy/systemd/claude-admin.service`, `deploy/scripts/deploy.sh`, `deploy/scripts/install_postgres.sh` (idempotent), `migrations/` directory + `sqlx-cli` config, `Makefile` deploy target, `README.md` Deploy section
- Modified: `crates/server/src/main.rs` (skipped — migrations run via sqlx CLI in deploy.sh, not in-process)
- Out of scope: zero-downtime blue/green, secrets manager (use `.env` on the VPS for v1), monitoring/alerting

**Motivation.** Self-hosting is a hard requirement and the riskiest infra step. Proving the deploy works on an empty server before adding more code keeps the deploy story always-known-good and protects later milestones from inheriting a broken pipeline.

**Validation.**
- On a fresh Ubuntu VPS, `make deploy HOST=user@host` succeeds and `/healthz` returns 200 over HTTPS at the configured domain
- Re-running `make deploy` produces no change to live behavior; in-flight request completes during restart
- `sqlx migrate run` is idempotent
- Caddy redirects non-HTTPS requests (301 to HTTPS) and serves a valid Let's Encrypt cert
- README "Deploy" section walks through the steps and is verified end-to-end

**Test scenarios.**
- _unit_ — **deploy_script_idempotent**: in a Docker container simulating a fresh Ubuntu, run `deploy.sh` twice; second run exits 0 with no changes
- _integration_ — **caddy_redirects_http_to_https**: docker-compose with Caddy + server, `curl http://...` → 301 to HTTPS
- _e2e_ — **fresh_vps_deploy_smoke**: documented manual reproduction (full automation lands in M0c when docker-e2e infra arrives); README must include the exact commands and observed `/healthz` response

**Blockers:** M0a-T2 merged; M0a-T3 merged
**Estimated LOC:** ~150

---

### M0a-T5 · Quota tracker stub: schema + endpoints

**Deliverable.** Postgres table `quota_usage(daemon_id text, period_start timestamptz, tokens_used bigint, primary key (daemon_id, period_start))`. Endpoints: `POST /api/quota/report` (daemon-authenticated, body `{tokens_used, period_start}`) upserts a row; `GET /api/quota/status` returns `{percent_used, threshold_warn: 0.8, threshold_pause: 1.0, paused: bool}` aggregating across daemons against a configured monthly limit. No real claude CLI integration yet — endpoint accepts any int.

**Expectation.** After this PR: `curl -XPOST /api/quota/report` accepts a token-usage report and stores it. `GET /api/quota/status` returns the aggregate, with `paused=true` flipping at 100%. The schema and endpoint shape are fixed so M6 can plug in real numbers without migration churn.

**Scope.**
- New: `migrations/0001_quota_usage.sql`, `crates/server/src/quota.rs` (handlers + service + types)
- Modified: `crates/server/src/main.rs` (mount routes), `crates/server/src/config.rs` (add `MONTHLY_TOKEN_LIMIT` default for the stub)
- Out of scope: real claude CLI usage tracking, dispatch pause logic that actually halts jobs (that lands in M6/M8), per-role accounting, daemon-side reporting (lands in M0b)

**Motivation.** Quota safety is the single point of failure in the all-CLI runtime model. Scaffolding the schema + endpoint shape now means M6 plugs real values in with zero migration churn. Setting the contract early avoids last-minute UX retrofits.

**Validation.**
- `POST /api/quota/report` upserts on `(daemon_id, period_start)` (second POST with same key updates, doesn't insert)
- `GET /api/quota/status` returns `percent_used = SUM(tokens_used) / MONTHLY_TOKEN_LIMIT` aggregated across daemons in the current month
- `paused` is true when `percent_used >= 1.0`, false otherwise
- `POST /api/quota/report` without a daemon bearer token → 401 (real daemon auth lands in M0b; this PR uses a stub bearer-token check)

**Test scenarios.**
- _unit_ — **quota_aggregation_handles_multiple_daemons**: insert rows from 2 daemon_ids, GET /status returns sum across both
- _integration_ — **report_endpoint_upserts**: POST same `(daemon_id, period_start)` twice, table has one row with the last value
- _integration_ — **paused_flag_at_100_percent**: insert `tokens_used = MONTHLY_TOKEN_LIMIT`, GET /status returns `paused=true`
- _integration_ — **unauthenticated_report_rejected**: POST without Authorization header → 401

**Blockers:** M0a-T2 merged
**Estimated LOC:** ~150

---

## Notes for the coder agent

- Stack: Rust + axum + sqlx · Postgres · React+Vite (later) · `claude` CLI as agent runtime (later).
- Each task is roughly one PR. Open as **draft**, push commits, request review when validation is fully satisfied.
- Tests for the code in a task ship in the same PR.
- Task IDs are stable; reference them in PR titles and commit messages: e.g., `[M0a-T1] add Cargo workspace + CI gates`.

## Dependency order

```
M0a-T1 ──┬──> M0a-T2 ──┬──> M0a-T3 ──> M0a-T4
         │             └──> M0a-T5
         └─ (T2 alone unblocks T5)
```

Parallel-safe pairs: T3 + T5 can proceed concurrently after T2 lands.
