FROM centos:7
ARG ARCH
RUN yum update -y
RUN yum groupinstall -y 'Development Tools'
RUN yum install -y centos-release-scl epel-release openssl-devel zip
RUN yum-config-manager --enable epel
RUN yum install -y llvm-toolset-7.0
RUN scl enable llvm-toolset-7.0 bash

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > rustup.sh
RUN /bin/sh rustup.sh --default-host ${ARCH}-unknown-linux-gnu --default-toolchain nightly -y
ENV PATH=/root/.cargo/bin:$PATH
RUN cargo install rls || true

RUN mkdir /stream-logs-to-s3
COPY ["Cargo.toml", "Cargo.lock", "/stream-logs-to-s3/"]
COPY src /stream-logs-to-s3/src
WORKDIR /stream-logs-to-s3
RUN cargo build --target ${ARCH}-unknown-linux-gnu
RUN cargo build --target ${ARCH}-unknown-linux-gnu --release
RUN VERSION=$(egrep '^version = ' Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/') && \
ln target/${ARCH}-unknown-linux-gnu/debug/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-el7-${ARCH}-debug && \
ln target/${ARCH}-unknown-linux-gnu/release/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-el7-${ARCH}-release && \
zip -9 stream-logs-to-s3-${VERSION}-el7-${ARCH}.zip stream-logs-to-s3-${VERSION}-el7-${ARCH}-debug stream-logs-to-s3-${VERSION}-el7-${ARCH}-release
