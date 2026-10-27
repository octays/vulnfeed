# Build stage for frontend
FROM oven/bun:1-alpine AS frontend-build

WORKDIR /app

# Copy package files first for better caching
COPY assets/package.json assets/bun.lock ./

# Install dependencies - this will be cached if package files don't change
RUN bun install --frozen-lockfile

# Copy source code after dependencies are installed
COPY assets/ .

# Build frontend
RUN bun run build

# Build stage for backend with better caching
FROM rust:1.89-alpine AS backend-build

# Change to domestic mirror for faster apk installs
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.tuna.tsinghua.edu.cn/g' /etc/apk/repositories

# Install system dependencies
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    git

WORKDIR /app

# Configure domestic mirror for faster crate downloads
RUN mkdir -p .cargo && \
    echo '[source.crates-io]' > .cargo/config.toml && \
    echo 'replace-with = "tuna"' >> .cargo/config.toml && \
    echo '[source.tuna]' >> .cargo/config.toml && \
    echo 'registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"' >> .cargo/config.toml

# Copy Cargo files first for better caching
COPY Cargo.toml Cargo.lock build.rs ./

# Copy source code
COPY src/ src/

# Copy migrations
COPY migrations/ migrations/

# Copy frontend build output; build.rs stages it into public/ where rust-embed picks it up
COPY --from=frontend-build /app/public/ assets/public/

# Build the application
RUN cargo build --release

# Production stage
FROM alpine:latest

# Change to domestic mirror for faster apk installs
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.tuna.tsinghua.edu.cn/g' /etc/apk/repositories

# Install runtime dependencies
RUN apk add --no-cache ca-certificates wget curl tzdata  \
    && cp /usr/share/zoneinfo/Asia/Shanghai /etc/localtime \
    && echo "Asia/Shanghai" > /etc/timezone

WORKDIR /app

# Copy binary
COPY --from=backend-build /app/target/release/vulnfeed ./vulnfeed
RUN chmod +x ./vulnfeed

# Copy default config
COPY dev/config.toml.example ./config.toml

# Expose port
EXPOSE 9000

# Run the application
CMD ["./vulnfeed", "server", "--config-file", "config.toml"]
