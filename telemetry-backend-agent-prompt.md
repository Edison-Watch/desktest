# Desktest Telemetry Backend — Agent Prompt

You are building an open-source telemetry backend for the **desktest** CLI tool. This is a new repository called `desktest-telemetry`. The backend receives anonymous usage stats and optional rich diagnostics (trajectories, screenshots) from desktest CLI users who opt in.

## Project Setup

- **Repo name:** `desktest-telemetry`
- **Language:** Rust (edition 2024)
- **Framework:** Axum 0.8
- **Database:** PostgreSQL (via `sqlx` with compile-time checked queries)
- **Object storage:** S3-compatible (Railway Storage Buckets, use `aws-sdk-s3` or `rust-s3`)
- **Deployment target:** Railway.com Hobby plan ($5/mo)
- **License:** MIT

### Dependencies to use

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
axum = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "chrono"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["limit", "cors"] }
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
dotenvy = "0.15"
aws-sdk-s3 = "1"
aws-config = "1"
```

## Architecture

```
src/
├── main.rs          # Axum server setup, router, startup
├── routes/
│   ├── mod.rs
│   ├── events.rs    # POST /api/events
│   ├── upload.rs    # POST /api/upload
│   └── health.rs    # GET /api/health
├── models.rs        # TelemetryEvent, Upload structs
├── validation.rs    # Input validation (UUID, semver, enums)
├── rate_limit.rs    # In-memory rate limiting (per-IP + per-install_id)
├── storage.rs       # S3 bucket operations (upload, delete, size check)
├── db.rs            # Database queries and pool setup
└── cleanup.rs       # 90-day retention cleanup logic
migrations/
├── 001_create_events.sql
└── 002_create_uploads.sql
Dockerfile
.env.example
```

## API Endpoints

### `GET /api/health`
- Returns `200 OK` with `{"status": "ok"}`
- Used by Railway health checks

### `POST /api/events`
- **Purpose:** Receive batched anonymous telemetry events
- **Content-Type:** `application/json`
- **Max body size:** 64KB (enforced by tower-http `RequestBodyLimitLayer`)
- **Request body:**
```json
{
  "events": [
    {
      "timestamp": "2026-03-24T14:32:45Z",
      "desktest_version": "0.12.1",
      "install_id": "550e8400-e29b-41d4-a716-446655440000",
      "event_type": "test_completed",
      "command": "run",
      "app_type": "appimage",
      "evaluator_mode": "hybrid",
      "provider": "anthropic",
      "model": "claude-sonnet-4-5-20250929",
      "status": "pass",
      "duration_ms": 45230,
      "agent_steps": 7,
      "error_category": null,
      "used_qa": false,
      "used_replay": false,
      "used_bash": false,
      "platform": "darwin"
    }
  ]
}
```
- **Validation rules (reject 400 if any fail):**
  - `install_id`: valid UUID v4 format
  - `desktest_version`: matches `\d+\.\d+\.\d+`
  - `event_type`: one of `test_completed`, `suite_completed`, `error`
  - `command`: one of `run`, `suite`, `attach`, `interactive`
  - `events` array: max 100 items per batch
- **Response:** `200 OK` on success, `400 Bad Request` on validation failure, `503 Service Unavailable` if storage cap exceeded

### `POST /api/upload`
- **Purpose:** Receive rich diagnostic tarballs (trajectories, screenshots)
- **Content-Type:** `multipart/form-data`
- **Max body size:** 50MB
- **Headers:**
  - `X-Install-Id`: UUID v4 (required)
  - `X-Run-Id`: UUID v4 (required)
- **Body:** Single file field named `archive` containing a `.tar.gz`
- **Validation:**
  - Both headers must be valid UUID v4
  - File must have `.tar.gz` extension
  - File size must be under 50MB
- **Behavior:**
  1. Upload tarball to S3 bucket at key `{install_id}/{run_id}.tar.gz`
  2. Insert metadata row into `uploads` table with `expires_at = NOW() + 90 days`
- **Response:** `200 OK` with `{"key": "..."}`, `400`/`503` on error

## Database Schema

```sql
-- migrations/001_create_events.sql
CREATE TABLE events (
    id SERIAL PRIMARY KEY,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    install_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    desktest_version TEXT,
    event_type TEXT NOT NULL,
    command TEXT,
    app_type TEXT,
    evaluator_mode TEXT,
    provider TEXT,
    model TEXT,
    status TEXT,
    duration_ms BIGINT,
    agent_steps INT,
    error_category TEXT,
    used_qa BOOLEAN,
    used_replay BOOLEAN,
    used_bash BOOLEAN,
    platform TEXT
);

CREATE INDEX idx_events_install_id ON events(install_id);
CREATE INDEX idx_events_received_at ON events(received_at);

-- migrations/002_create_uploads.sql
CREATE TABLE uploads (
    id SERIAL PRIMARY KEY,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    install_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    bucket_key TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_uploads_expires_at ON uploads(expires_at);
CREATE INDEX idx_uploads_install_id ON uploads(install_id);
```

## Rate Limiting

Implement in-memory rate limiting (no external dependency like Redis needed at this scale).

**Per-IP limits:**
- `/api/events`: 60 requests/minute
- `/api/upload`: 10 requests/minute

**Per-install_id limits:**
- Events: 100 events/day (across all batches)
- Uploads: 50 uploads/day

**Implementation:** Use a `DashMap<String, (u32, Instant)>` or similar concurrent map. Clean up stale entries periodically (every 5 minutes via tokio::spawn background task). Return `429 Too Many Requests` when limits exceeded.

## Storage Hard Caps (Circuit Breaker)

Prevent cost runaway even under sustained attack:

| Resource | Hard Cap | Estimated Max Cost |
|----------|----------|-------------------|
| PostgreSQL `events` table | 1GB (~5M rows) | $0.25/mo |
| S3 bucket total size | 100GB | $1.50/mo |

**Implementation:**
- Cache the current size estimate (refresh every 5 minutes via background task)
- For Postgres: `SELECT pg_total_relation_size('events')`
- For S3: maintain a running total in an `atomic u64` updated on each upload, periodically reconciled with actual bucket size
- When cap is reached, return `503 Service Unavailable` with body `{"error": "storage_cap_exceeded"}`

## 90-Day Retention Cleanup

Implement as a function callable from:
1. A background tokio task that runs once daily
2. A `POST /api/cleanup` endpoint (protected by a shared secret env var `CLEANUP_SECRET`) for manual triggering

**Cleanup logic:**
```
1. SELECT id, bucket_key FROM uploads WHERE expires_at < NOW() LIMIT 1000
2. For each: delete S3 object, then delete DB row
3. Repeat until no more expired rows
4. Optional: DELETE FROM events WHERE received_at < NOW() - INTERVAL '1 year'
```

## Environment Variables

```env
# .env.example
DATABASE_URL=postgres://user:pass@host:5432/desktest_telemetry
AWS_ENDPOINT_URL=https://your-bucket.railway.app  # Railway S3-compatible endpoint
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
S3_BUCKET_NAME=desktest-telemetry
CLEANUP_SECRET=some-random-secret
PORT=8080
RUST_LOG=info
```

## Dockerfile

```dockerfile
FROM rust:1.87-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY migrations/ migrations/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/desktest-telemetry /usr/local/bin/
EXPOSE 8080
CMD ["desktest-telemetry"]
```

## Implementation Order

1. **Scaffold project** — `cargo init`, add dependencies, create directory structure
2. **Database setup** — `db.rs` with sqlx pool, migration files
3. **Models + validation** — `models.rs`, `validation.rs` with all the rules above
4. **Health endpoint** — `GET /api/health` (proves server runs)
5. **Events endpoint** — `POST /api/events` with validation + DB insert
6. **Rate limiting** — `rate_limit.rs` middleware, wire into router
7. **Upload endpoint** — `POST /api/upload` with S3 storage
8. **Storage caps** — Background task for size monitoring, circuit breaker logic
9. **Cleanup** — 90-day retention cleanup task + manual endpoint
10. **Dockerfile** — Build and test locally with `docker compose` (Postgres + app)
11. **Deploy to Railway** — Push, configure env vars, add Postgres plugin + Storage Bucket

## Testing

- Unit tests for validation logic (UUID format, semver, enum matching)
- Unit tests for rate limit counter logic
- Integration tests with a test Postgres database (use `sqlx::test` or testcontainers)
- Integration test for the full `/api/events` flow (valid + invalid payloads)
- Integration test for `/api/upload` (mock S3 or use local MinIO)
- Test that oversized payloads are rejected before processing
- Test that rate limits return 429
- Test that storage cap returns 503

## Key Design Decisions

- **No authentication** — This is anonymous telemetry. Rate limits + caps are sufficient. An API key adds management overhead for no real security benefit.
- **Batch events** — The CLI sends all events for a run in a single POST, reducing round trips.
- **Fail-open on the CLI side** — If the backend is down, the CLI silently drops events. Telemetry must never break the user's workflow.
- **S3 for blobs, Postgres for metadata** — S3 bucket egress is free on Railway, making it ideal for large files. Postgres handles structured queries efficiently.
- **In-memory rate limiting** — At this scale (single instance), no need for Redis. A concurrent hashmap is sufficient and adds zero operational overhead.
