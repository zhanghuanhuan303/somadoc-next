//! Print the canonical fixture JSON. Used by `scripts/wasm-hash.sh`.

fn main() {
    print!("{}", soma_wasm::fixture_json());
}
