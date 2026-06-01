FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

FROM scratch
COPY --from=builder /src/target/release/paste /paste
EXPOSE 3000
ENTRYPOINT ["/paste"]
