# wokwi-rust-demo

```sh
rustup target add thumbv6m-none-eabi
cargo install elf2uf2-rs
cargo build --release
elf2uf2-rs target/thumbv6m-none-eabi/release/wokwi-test target/thumbv6m-none-eabi/release/wokwi-test.uf2
```
