# whelk-rs

This is an experimental implementation of [Whelk](https://github.com/balhoff/whelk) in Rust

## Build

Clone this repo, change into the whelk-rs directory (```cd whelk-rs```), and then:
```shell
cargo build --release
```

## Running
```shell
./target/release/whelk -i uberon.owl
```

## Testing
```shell
cargo test --release -- --nocapture
```
