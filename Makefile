default: build

test: build
	cargo test --all --tests

build:
	cargo rustc --manifest-path=team-vesting-lockup/Cargo.toml --crate-type=cdylib --target=wasm32-unknown-unknown --release

	mkdir -p target/wasm32-unknown-unknown/optimized
	soroban contract optimize \
		--wasm target/wasm32-unknown-unknown/release/team_vesting_lockup.wasm \
		--wasm-out target/wasm32-unknown-unknown/optimized/team_vesting_lockup.wasm
	cd target/wasm32-unknown-unknown/optimized/ && \
		for i in *.wasm ; do \
			ls -l "$$i"; \
		done

fmt:
	cargo fmt --all

clean:
	cargo clean

