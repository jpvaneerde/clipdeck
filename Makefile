# Common tasks. `make dev` runs the server on :8080 and Vite on :5173.
DATA ?= ./data

.PHONY: wasm web server library index dev test image

wasm:
	./scripts/build-wasm.sh

web: wasm
	cd web && npm ci && npm run build

server:
	cargo build --release -p clipdeck-server -p clipdeck-samplegen

# Generate the sample library (~2,800 samples, ~1.7 GB). SCALE=0.1 for a quick one.
library: server
	./target/release/clipdeck-samplegen --out $(DATA)/library --scale $(or $(SCALE),1.0)

index: server
	CLIPDECK_DATA=$(DATA) ./target/release/clipdeck-server index

dev: wasm server
	CLIPDECK_DATA=$(DATA) CLIPDECK_WEB=web/dist ./target/release/clipdeck-server & \
	cd web && npm install && npm run dev

test:
	cargo test --workspace
	cd web && npm run typecheck

image:
	docker build -t ghcr.io/jpvaneerde/clipdeck:dev .
