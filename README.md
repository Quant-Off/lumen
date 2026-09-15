# Lumen

[![Language](https://img.shields.io/badge/README-Korean_Ver-blue?style=for-the-badge)](README_KR.md)
[![Lumen-Ver](https://img.shields.io/badge/Lumen_Milestone-v0.5.0-000000?style=for-the-badge)](https://github.com/Quant-Off/)
[![Qu4nt-Space-Discord](https://img.shields.io/badge/Qu4nt_Space-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/9utg4hp3m8)

Lumen is a high-security, verifiable Rust AI agent framework for zero-trust and air-gapped environments. It combines permission control (WASM sandbox) with result verification (zkML) to enable safe, autonomous AI operation that meets the strict security standards required by government-grade deployments.

Every model, policy file and inference engine binary is pinned by a BLAKE3 hash before it is used. Agents run inside a wasmtime sandbox and reach tools only through Ed25519-signed capabilities. Tool routing decisions are bound to a commitment that can later be verified. All third-party sources are vendored, so the whole workspace builds offline.

## Documentation

- [USAGE.md](USAGE.md): quick start, CLI subcommands, workspace layout, feature flags, verification gates and current limitations
- [INTRODUCTION.md](INTRODUCTION.md): design philosophy, the four security pillars, threat model and roadmap
- [PRACTICE_LLM.md](PRACTICE_LLM.md): running a real LLM under Lumen with the llama.cpp `llama-server` backend
- [INFERENCE.md](INFERENCE.md): inference engine selection, trust boundaries and how to add another engine

## Quick Start

```bash
$ cargo build --workspace
$ cargo test  --workspace
$ cargo run -p lumen-cli --example hello_agent
```

The example walks through echo, add and jailbreak blocking in three agent steps. See [USAGE.md](USAGE.md) for the full CLI.

## License

Dual-licensed under Apache-2.0 OR MIT; users may freely choose either. See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.

## Contributing

Lumen is intended as a contribution to the AI and security open-source ecosystem.
