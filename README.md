# CadCraft in Rust (test version)

A test version of [CadCraft](https://getcadcraft.com) written in Rust: the same apps, files and help as the
JavaScript CadCraft 0.5, rebuilt on one Rust codebase for the desktop and the browser (WebAssembly + WebGPU).
It is a test version; CadCraft itself is the JavaScript build.

Preview: https://rust.getcadcraft.com (unlisted).

Status: see [ROADMAP.md](ROADMAP.md) and [docs/roadmap.md](docs/roadmap.md).

## Build

```sh
cargo xtask ci                 # the quality gate
cargo run -p cadcraft-cli -- info drawing.dwg
```

The toolchain is pinned in `rust-toolchain.toml`. Agents: read [AGENTS.md](AGENTS.md).

## Licence

Apache-2.0. See [LICENSE](LICENSE), [NOTICE](NOTICE) and [THIRD_PARTY.md](THIRD_PARTY.md).
