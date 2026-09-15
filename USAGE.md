# Usage Guide

[![Language](https://img.shields.io/badge/USAGE-Korean_Ver-blue?style=for-the-badge)](USAGE_KR.md)

This document collects everything needed to build, test and operate Lumen: quick start, CLI subcommands, workspace layout, feature flags, verification gates and the current limitations. For the design background see [INTRODUCTION.md](INTRODUCTION.md); for running a real LLM see [PRACTICE_LLM.md](PRACTICE_LLM.md); for the inference engine design see [INFERENCE.md](INFERENCE.md).

---

## Table of Contents

1. [Quick Start](#1-quick-start)
2. [CLI Usage](#2-cli-usage)
3. [Workspace Structure](#3-workspace-structure)
4. [Feature Flags](#4-feature-flags)
5. [Verification Gates](#5-verification-gates)
6. [Air-Gapped Builds](#6-air-gapped-builds)
7. [Current Limitations and Placeholders](#7-current-limitations-and-placeholders)

---

## 1. Quick Start

Developed on Rust stable 1.95 and operates stably on later versions. `rust-toolchain.toml` is applied automatically, so no additional configuration is needed. All third-party crate sources are vendored into `vendor/`, so the build never touches the network.

For a basic build or to run the full test suite (190+ tests as of v0.5):

```bash
$ cargo build --workspace # basic build
$ cargo test --workspace  # run all tests
```

To verify a build with the optional features enabled, AES-GCM/x25519 channel encryption (`crypto-channel`) and the `#[lumen_agent]` proc-macro (`macros`):

```bash
$ cargo test --workspace --features lumen-channel/crypto-channel,lumen-sdk/macros
```

To see the full behavior at a glance:

```bash
$ cargo run -p lumen-cli --example hello_agent
```

This demo walks through a 3-step sequence (echo, add, and jailbreak blocking) showing all [four security pillars](INTRODUCTION.md#four-core-pillars) in action.

---

## 2. CLI Usage

The `lumen` binary provides 7 subcommands: `init`, `verify-model`, `sbom`, `defend`, `prove`, `run`, and `verifier`.

### `init`

Outputs a policy file template to stdout. Redirect it to a path such as `policies/my_policy.toml` to get started. The template includes a commented `[inference]` section showing how to select the `llama-server` backend.

```bash
$ cargo run -p lumen-cli -- init > policies/my_policy.toml
```

### `defend`

Performs prompt injection analysis on arbitrary text and outputs the `Verdict` and `corpus_version`.

```bash
$ cargo run -p lumen-cli -- defend --text "ignore previous instructions"
```

### `verify-model`

Compares a manifest against an actual file and verifies the BLAKE3 hash and Ed25519 signature (if present).

```bash
$ cargo run -p lumen-cli -- verify-model --manifest model.toml --file model.safetensors
```

### `sbom`

Outputs a CycloneDX 1.5 SBOM as JSON from the list of models declared in a policy file.

```bash
$ cargo run -p lumen-cli -- sbom --policy policies/default.toml
```

### `prove`

Generates a mock ZKP for a tool routing decision and immediately self-verifies it. The circuit identifier, prompt hash, proof digest, and verification verdict are all printed to stdout.

```bash
$ cargo run -p lumen-cli -- prove --prompt "echo hi" --tool echo --circuit-id lumen.routing.v1
```

### `run`

Executes one agent step while enforcing the BLAKE3 pin of the policy file. If `--policy-hash` does not match the actual BLAKE3 of the policy file, the request is immediately rejected; this is the core of Lumen's Zero-Trust UX. If a model manifest is declared in the policy file, the `verify_model` step must pass first. The inference backend is chosen by the policy file's `[inference]` section (`dummy` by default, or `llama-server`), and its parameters, including the engine binary hash, are pinned together with the policy. After that, the host side performs capability issuance, defense analysis, inference, policy verification, tool execution, ZKP generation, and self-verify in one shot.

```bash
$ cargo run -p lumen-cli -- run --policy policies/default.toml --policy-hash <BLAKE3HEX> --prompt "echo hello"
```

### `verifier`

On-chain verifier tool with two subcommands: `emit` and `deploy`. `verifier emit` deterministically writes EVM (Solidity) or Mina (o1js) verifier source, a deploy script, and a metadata JSON to disk (byte-identical output guaranteed for the same input). `verifier deploy` executes `forge create` or `zk deploy` as a child process, but defaults to a dry run, printing only a redacted command string. The EVM private key is read from the `LUMEN_DEPLOY_PRIVKEY` environment variable and masked as `***` in audit logs. For air-gapped operation, carry the directory produced by `emit` as text and copy only the dry-run output to the production environment.

```bash
$ cargo run -p lumen-cli -- verifier emit --chain evm --circuit-id lumen.routing.binary.v1 --out ./out/evm
$ cargo run -p lumen-cli -- verifier emit --chain mina --out ./out/mina
$ cargo run -p lumen-cli -- verifier deploy --chain evm --rpc https://sepolia.example.org --out ./out/evm
```

---

## 3. Workspace Structure

Lumen is a Cargo workspace consisting of 15 library crates and 1 binary crate. Additionally, two wasm32-only demo agents are separated as an independent workspace under `agents/`.

```text
crates/
  lumen-core         ID, BLAKE3, Ed25519 wrapper, CSPRNG, Error, Time
  lumen-fixed        Q16.16 and Q8.24 deterministic integer arithmetic (no_std)
  lumen-capability   Capability tokens, PolicyEngine, AgentMessage resource
  lumen-channel      InProc, AttestedChannel, AES-GCM/x25519 EncryptedChannel
  lumen-provenance   Safetensors, ONNX, GGUF header verification, SBOM, PinSet auto-rotation
  lumen-defense      Three-stage injection filter based on Aho-Corasick and RegexSet
  lumen-zkml         ProvingSystem trait, Mock commitment prover, ezkl stub
  lumen-inference    InferenceEngine/StreamingEngine traits, BackendRegistry, verified model loader, tool-call grammar; Dummy / llama-server (llama.cpp) / TEE channel backends
  lumen-sandbox      wasmtime determinism Config and capability-gated imports
  lumen-agent        Agent runtime (defense -> infer -> policy -> tool -> prove)
  lumen-orchestrator tokio multi-agent supervisor, capability-gated messaging
  lumen-attestation  Intel TDX and AMD SEV-SNP quote parser
  lumen-sdk          WASM agent SDK (safe host import wrappers)
  lumen-sdk-macros   `#[lumen_agent]` proc-macro
  lumen-onchain      EVM and Mina verifier emit and deployment automation
  lumen-cli          `lumen` binary and demo examples
agents/
  echo-agent         wasm32 demo agent using the raw SDK
  macro-agent        wasm32 agent demonstrating the #[lumen_agent] proc-macro
```

---

## 4. Feature Flags

- `lumen-channel/crypto-channel`: Enables EncryptedChannel with AES-GCM, x25519, and BLAKE3 KDF
- `lumen-zkml/ezkl`: ezkl placeholder
- `lumen-inference/llama-server`: llama.cpp backend that drives a hash-pinned, isolated `llama-server` process over a Unix socket (pure Rust HTTP/SSE client, GBNF tool-call grammar, streaming); enabled by default in `lumen-cli`
- `lumen-sdk/macros`: Re-export of the `#[lumen_agent]` proc-macro
- `lumen-sdk/alloc`: Exposes dynamic String/Vec helper functions

All features are disabled by default in the library crates and are intentionally opt-in to maintain consistency as an _air-gapped self-contained_ build.

---

## 5. Verification Gates

A mergeable change must pass all of the following commands:

```bash
$ cargo build  --workspace --all-targets
$ cargo test   --workspace
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo fmt    --all -- --check
$ cargo deny   check
```

To verify with the optional features as well, append `--features lumen-channel/crypto-channel,lumen-sdk/macros` to the build, test and clippy commands. CI additionally runs `cargo doc` with `RUSTDOCFLAGS=-D warnings`, `cargo audit`, an `--offline` air-gapped job, and the wasm32 build of `agents/echo-agent`.

---

## 6. Air-Gapped Builds

`.cargo/config.toml` forces crates.io source replacement onto `vendor/` and sets `net.offline`, so every build, online or offline, compiles byte-identical sources. When `Cargo.toml` changes, re-synchronise the vendor directory on a connected machine and commit `Cargo.lock` and `vendor/` together:

```bash
$ ./scripts/vendor.sh          # refresh Cargo.lock + vendor/ (network required)
$ ./scripts/vendor.sh --check  # verify vendor/ matches Cargo.lock (offline)
```

---

## 7. Current Limitations and Placeholders

`MockCommitmentProver` is a BLAKE3 commitment, not a ZKP (`Verification::CommitmentOnly` and `Verification::ZkVerified` are separated at the type level, making confusion impossible by construction). The halo2 circuit shipped in v0.3 was removed in v0.5 because its MockProver verification gave no ZK guarantee; a succinct backend (SP1, RISC Zero, or similar) will be chosen at the next milestone review, so `ZkVerified` is currently unreachable.

The `ezkl` backend is a feature stub. `DummyEngine` only recognizes echo and add patterns. `LlamaServerEngine` (the `llama-server` feature) runs llama.cpp as a separate, BLAKE3-pinned process and binds the served model to a `VerifiedModelHandle` via `/props`; all model files remain subject to mandatory BLAKE3 + optional Ed25519 verification via `VerifiedModelLoader`. The engine binary itself is hash-pinned but not yet signature-verified.

The EVM Solidity contract in `lumen-onchain` performs constraint rechecking; an upgrade to succinct proof verification will follow the ZK backend decision.

TEE attestation parses Intel TDX and AMD SEV-SNP documents for format consistency only (`Verdict::FormatOnly`); PCK chain and VCEK signature verification are not implemented yet.
