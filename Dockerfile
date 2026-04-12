# ========================
# Build profiles:
#
# Production (hardcoded semerkant URL, no env override):
#   docker build -t alacahoyuk .
#
# Dev/testing (reads SEMERKANT_URL from env):
#   docker build --build-arg FEATURES="" -t alacahoyuk:dev .
#
# Custom semerkant URL baked in:
#   (edit the obfstr! URL in src/lib.rs, then build with default features)
#
# Run with:
#   docker run --rm \
#       -e LICENSE_KEY=LICENSE_KEY \
#       -e SEMERKANT_URL=http://host.docker.internal:3001/hub/v1 \
#       -p 3000:3000 \
#       -v alacahoyuk_data:/opt/alacahoyuk/data \
#       alacahoyuk:dev
# ========================

# ========================
# Stage 1: Builder
# ========================
FROM rust:1.93.0-alpine3.21 AS builder

ARG FEATURES="hardcoded-semerkant-url"

RUN apk add --no-cache musl-dev pkgconf openssl-dev openssl-libs-static perl make

WORKDIR /app

# Cache dependencies (dummy main + lib trick)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    echo "" > src/lib.rs && \
    cargo build --release $([ -n "$FEATURES" ] && echo "--features $FEATURES") && \
    rm -rf src

# Copy source and build
COPY src src
COPY migrations migrations
RUN touch src/main.rs src/lib.rs && \
    cargo build --release $([ -n "$FEATURES" ] && echo "--features $FEATURES")

# ========================
# Stage 2: Runtime
# ========================
FROM alpine:3.21

RUN apk add --no-cache \
    libssl3 \
    ca-certificates \
    wget

# Non-root user
RUN addgroup -g 1000 orda && \
    adduser -u 1000 -G orda -s /bin/sh -D orda

# Data + TLS directories
RUN mkdir -p /opt/alacahoyuk/data /opt/alacahoyuk/tls && \
    chown -R orda:orda /opt/alacahoyuk

WORKDIR /app
COPY --from=builder /app/target/release/alacahoyuk ./alacahoyuk
RUN chown orda:orda ./alacahoyuk

USER orda

ENV DATABASE_URL=sqlite:///opt/alacahoyuk/data/tumulus.db
ENV RUST_LOG=info
ENV PORT=3000

EXPOSE 3000
VOLUME ["/opt/alacahoyuk/data"]

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD TOKEN=$(echo -n "$LICENSE_KEY" | sha256sum | cut -d' ' -f1) && \
        wget -qO- --header="Authorization: Bearer $TOKEN" http://localhost:${PORT}/health || exit 1

CMD ["./alacahoyuk"]
