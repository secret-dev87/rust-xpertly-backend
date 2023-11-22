FROM rust:latest AS planner
WORKDIR /app
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust:latest AS builder
WORKDIR /app
RUN apt-get update && apt-get install build-essential -y && apt install gcc-x86-64-linux-gnu -y
RUN cargo install cargo-chef
RUN rustup target add x86_64-unknown-linux-gnu

ENV RUSTFLAGS='-C linker=x86_64-linux-gnu-gcc'
ENV CC_x86_64_unknown_linux_gnu=/usr/bin/x86_64-linux-gnu-gcc
ENV RUSTC_LINKER=/usr/bin/x86_64-linux-gnu-gcc

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --target x86_64-unknown-linux-gnu --recipe-path recipe.json 

COPY . .
RUN cargo build -p api --bin api --target=x86_64-unknown-linux-gnu

FROM --platform=linux/amd64 debian:buster-slim AS runtime
WORKDIR /app

COPY /certs/*.crt /usr/local/share/ca-certificates/

RUN apt-get update
RUN apt-get install -y ca-certificates
RUN update-ca-certificates

EXPOSE 8000
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/debug/api /usr/local/bin
# COPY --from=builder /app/Rocket.toml /etc/xpertly/Rocket.toml
# ENV ROCKET_CONFIG=/etc/xpertly/Rocket.toml
# ENV ROCKET_PORT=8001

CMD ["/usr/local/bin/api"]