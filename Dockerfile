# ── Builder ──────────────────────────────────────────────────────────────
FROM rust:1.88-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY USER.md WORKER.md ./

RUN cargo build --release

# ── Runtime ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash app

COPY --from=builder /app/target/release/ai-assist /usr/local/bin/ai-assist

USER app
WORKDIR /home/app

EXPOSE 8080

ENV DISABLE_CLI=true
ENV AI_ASSIST_WS_PORT=8080

CMD ["ai-assist"]
