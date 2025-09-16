FROM rust:1.70 as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY migrations ./migrations

# Build application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -m -u 1001 app

# Create data directory
RUN mkdir -p /data && chown app:app /data

WORKDIR /app

# Copy binary from builder stage
COPY --from=builder /app/target/release/oucc-kizai-bot ./
COPY --from=builder /app/migrations ./migrations

# Change ownership
RUN chown -R app:app /app

USER app

# Set environment variables
ENV DATABASE_URL=sqlite:/data/bot.db
ENV LOG_LEVEL=info

# Expose no ports (Discord bot doesn't need to listen)

VOLUME ["/data"]

CMD ["./oucc-kizai-bot"]