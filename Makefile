VERSION := $(shell egrep '^version = ' Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/')
SRCS = src/*.rs
PLATFORMS = amzn2 el7 musl
ZIP_FILES = ${PLATFORMS:%=stream-logs-to-s3-${VERSION}-%.zip}

all: ${ZIP_FILES}
macos: stream-logs-to-s3-${VERSION}-macos.zip

clean:
	rm -f stream-logs-to-s3-*.zip

stream-logs-to-s3-${VERSION}-%.zip: %.dockerfile ${SRCS} Cargo.toml Cargo.lock
	docker build --progress=plain --tag stream-logs-to-s3:${VERSION}-$* -f $*.dockerfile .
	docker run --rm stream-logs-to-s3:${VERSION}-$* cat stream-logs-to-s3-${VERSION}-$*.zip > stream-logs-to-s3-${VERSION}-$*.zip

stream-logs-to-s3-${VERSION}-macos.zip: ${SRCS} Cargo.toml Cargo.lock
	@if [[ "$(shell uname -s)" != "Darwin" ]]; then echo "You must run this on a Mac to build the Mac target: $(shell uname -s)" 1>&2; exit 1; fi
	cargo build --target x86_64-apple-darwin
	cargo build --target x86_64-apple-darwin --release
	cd target/x86_64-apple-darwin/debug && gzip < stream-logs-to-s3 > ../../stream-logs-to-s3-${VERSION}-macos-debug.gz
	cd target/x86_64-apple-darwin/release && gzip < stream-logs-to-s3 > ../../stream-logs-to-s3-${VERSION}-macos-release.gz
	cd target && zip -0 ../stream-logs-to-s3-${VERSION}-macos.zip stream-logs-to-s3-${VERSION}-macos-debug.gz stream-logs-to-s3-${VERSION}-macos-release.gz

upload:
	mkdir upload-temp && cd upload-temp && for file in ../stream-logs-to-s3-${VERSION}-*.zip; do unzip $${file}; done && aws --profile iono s3 sync . s3://dist.ionosphere.io/bin/ && cd .. && rm -rf upload-temp

.PHONY: all clean macos
