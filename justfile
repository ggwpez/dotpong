set quiet

export SKIP_PALLET_REVIVE_FIXTURES := "1"
export SKIP_WASM_BUILD := "1"
export AR := "/opt/homebrew/opt/llvm@17/bin/llvm-ar"
export CC := "clang"
export CXX := "clang++"

check:
	cargo check -q

run network *args:
	cargo run -q -- --network {{network}} {{args}}

backfill network *args:
	cargo run -q --bin backfill -- --network {{network}} {{args}}
