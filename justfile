set quiet

export SKIP_PALLET_REVIVE_FIXTURES := "1"
export SKIP_WASM_BUILD := "1"
export AR := "/opt/homebrew/opt/llvm@17/bin/llvm-ar"
export CC := "clang"
export CXX := "clang++"

check:
	cargo check -q

run network *args:
	cargo run -r -q -- --network {{network}} {{args}}

backfill network *args:
	cargo run -r -q --bin backfill -- --network {{network}} {{args}}

paseo:
	cargo run -r -q -- --network paseo-hub --delay 10

fetch-db network:
	scp server2:/home/vados/dotpong/dotpong-{{network}}.db ./
