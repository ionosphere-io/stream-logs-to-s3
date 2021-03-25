FROM centos:7
RUN yum update -y
RUN yum groupinstall -y 'Development Tools'
RUN yum install -y centos-release-scl epel-release openssl-devel zip
RUN yum-config-manager --enable epel
RUN yum install -y llvm-toolset-7.0
RUN scl enable llvm-toolset-7.0 bash

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > rustup.sh
RUN /bin/sh rustup.sh --default-host x86_64-unknown-linux-gnu --default-toolchain nightly -y
ENV PATH=/root/.cargo/bin:$PATH

RUN mkdir /stream-logs-to-s3
COPY ["Cargo.toml", "Cargo.lock", "/stream-logs-to-s3/"]
COPY src /stream-logs-to-s3/src
WORKDIR /stream-logs-to-s3
RUN cargo build --target x86_64-unknown-linux-gnu
RUN cargo build --target x86_64-unknown-linux-gnu --release
WORKDIR target
RUN zip ../stream-logs-to-s3.zip x86_64-unknown-linux-gnu/debug/stream-logs-to-s3 x86_64-unknown-linux-gnu/release/stream-logs-to-s3
WORKDIR ..
