# Stage 1: Build headless at-pc-server binary
FROM rust:bookworm AS server-builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY media/ media/

# Build pure headless server with zero desktop GUI dependencies
RUN cargo build --release -p at-pc-server --no-default-features --locked

# Stage 3: Minimal runtime container
FROM debian:bookworm-slim AS runner
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /var/lib/at-pc /var/log/at-pc /etc/at-pc

COPY --from=server-builder /app/target/release/at-pc-server /usr/local/bin/at-pc-server
COPY server_config.example.toml /etc/at-pc/server_config.toml

ENV AT_PC_DATA_DIR=/var/lib/at-pc \
    AT_PC_LOG_FILE=/var/log/at-pc/at-pc-server.log

EXPOSE 9800 9801

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:9800/health || exit 1

ENTRYPOINT ["/usr/local/bin/at-pc-server"]
CMD ["--host", "0.0.0.0", "--ws-port", "9801", "--mcp-port", "9800", "--meta-file", "/var/lib/at-pc/terminals_meta.json", "--audit-file", "/var/log/at-pc/audit.jsonl"]
