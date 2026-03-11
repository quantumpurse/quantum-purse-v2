#!/usr/bin/env bash
if [[ "${1:-}" == "--release" ]]; then
	open "target/release/qpv2.app"
else
	./target/debug/qpv2.app/Contents/MacOS/qpv2-gui
fi
