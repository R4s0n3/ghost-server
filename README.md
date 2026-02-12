# ghost-api server (Rust rewrite)

This repo now runs the API server as a Rust/Axum service.

## What stayed the same

- HTTP routes and response contracts from the previous Bun/Express server
- Convex backend functions in `convex/` (Convex functions still run in TypeScript)
- Ghostscript + pdfinfo processing workflow for PDF preflight and grayscale conversion
- Stripe + Clerk + API key auth integration points

## Run locally

```bash
cargo run --bin ghost-api-server
```

Server binds to `0.0.0.0:${PORT:-9001}`.

## Required environment variables

- `CONVEX_URL`
- `CLERK_SECRET_KEY` (needed for user sync middleware)
- `CLERK_ISSUER` (recommended; pins JWT issuer validation to your Clerk tenant)
- `STRIPE_SECRET_KEY` (required for Stripe endpoints)
- `STRIPE_WEBHOOK_SECRET` (required for `/api/stripe/webhook`)

## Optional environment variables

- `PORT`
- `TRUST_PROXY`
- `TLS_KEY_PATH`
- `TLS_CERT_PATH`
- `FRONTEND_URL`
- `GHOSTSCRIPT_CONCURRENCY` or `PROCESSING_CONCURRENCY`
- `LOG_GHOSTSCRIPT_TIMINGS`
- `LOG_TASK_QUEUE_TIMINGS`
- `STRIPE_PRICE_ID_STARTER`
- `STRIPE_PRICE_ID_PRO`
- `STRIPE_PRICE_ID_BUSINESS`
- `STRIPE_PRICE_ID_ENTERPRISE`

## Docker

Build and run with:

```bash
docker build -t ghost-api-server .
docker run --rm -p 9001:9001 --env-file .env ghost-api-server
```

The runtime image builds MuPDF from upstream source and enforces:
- `mutool` version `>= 1.26.8`
- `mutool recolor` command availability

This prevents grayscale `engine=mupdf` behavior from drifting across environments.
