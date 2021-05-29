#!/bin/sh

set -e
docker build -t crossbuild:local .
cross build --release --target armv7-unknown-linux-gnueabihf
scp ./target/armv7-unknown-linux-gnueabihf/release/memnarch-rs mirrodin:memnarch-rs/
