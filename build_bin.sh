#!/usr/bin/env bash
BIN="linch"
FEATURES=""

LINUX="./target/release/${BIN}"

cargo build --release $FEATURES

mkdir ./bin 2>/dev/null

cp "$LINUX" "./bin/${BIN}"
