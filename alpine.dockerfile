FROM alpine:3
RUN apk add --no-cache curl
RUN ls -la
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > rustup.sh
RUN /bin/sh rustup.sh --default-host x86_64-unknown-linux-musl --default-toolchain nightly -y
ENV PATH=/root/.cargo/bin:$PATH

RUN mkdir /stream-logs-to-s3
COPY ["Cargo.toml", "Cargo.lock", "/stream-logs-to-s3/"]
COPY src /stream-logs-to-s3/src
WORKDIR /stream-logs-to-s3
RUN apk add --no-cache clang gcc musl-dev openssl-dev openssl-libs-static zip
RUN cargo build --target x86_64-unknown-linux-musl
RUN cargo build --target x86_64-unknown-linux-musl --release
WORKDIR target
RUN zip ../stream-logs-to-s3.zip x86_64-unknown-linux-musl/debug/stream-logs-to-s3 x86_64-unknown-linux-musl/release/stream-logs-to-s3
WORKDIR ..
