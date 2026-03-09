#!/usr/bin/env bash
if [[ "${1:-}" == "--release" ]]; then
	open "target/release/qpkv.app"
else
	./target/debug/qpkv.app/Contents/MacOS/qpkv-gui
fi
