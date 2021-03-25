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
WORKDIR /stream-logs-to-s3/target/x86_64-unknown-linux-musl
RUN VERSION=$(egrep '^version = ' ../../Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/'); \
cd debug; gzip < stream-logs-to-s3 > ../../../stream-logs-to-s3-${VERSION}-musl-debug.gz; \
cd ../release; gzip < stream-logs-to-s3 > ../../../stream-logs-to-s3-${VERSION}-musl-release.gz
WORKDIR /stream-logs-to-s3
RUN VERSION=$(egrep '^version = ' Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/'); \
zip -0 stream-logs-to-s3-${VERSION}-musl.zip stream-logs-to-s3-${VERSION}-musl-debug.gz stream-logs-to-s3-${VERSION}-musl-release.gz
