# syntax=docker/dockerfile:1

FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /src

# Cache dependency compilation separately from source changes
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release
RUN rm -f target/release/deps/paste*

COPY src/ src/
RUN cargo build --release

FROM scratch
COPY --from=builder /src/target/release/paste /paste
EXPOSE 3000
ENTRYPOINT ["/paste"]
