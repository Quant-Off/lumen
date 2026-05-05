# Lumen

[![Language](https://img.shields.io/badge/README-Korean_Ver-blue?style=for-the-badge)](README.md)
[![Lumen-Ver](https://img.shields.io/badge/Lumen_Milestone-v0.4-000000?style=for-the-badge)](https://github.com/Quant-Off/)
[![Qu4nt-Space-Discord](https://img.shields.io/badge/Qu4nt_Space-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://github.com/Quant-Off/)

Lumen is a high-security, verifiable Rust AI agent framework for zero-trust and air-gapped environments. It combines permission control (WASM sandbox) with result verification (zkML) to enable safe, autonomous AI operation that meets the strict security standards required by government-grade deployments. For detailed design philosophy and technical background, refer to the [INTRODUCTION_EN.md](INTRODUCTION_EN.md) document.

## Quick Start

> [!TIP]
> To use Lumen’s LLM inference pipeline, please refer to the [PRACTICE_LLM_EN.md](PRACTICE_LLM_EN.md) document.

Developed on Rust stable 1.82 and operates stably on later versions. `rust-toolchain.toml` is applied automatically — no additional configuration is needed.

For a basic build or to run all 140+ tests as of [v0.4](INTRODUCTION_EN.md#roadmap):

```bash
$ cargo build --workspace # basic build
$ cargo test --workspace  # run all tests
```

To verify a build with all features enabled — AES-GCM/x25519 channel encryption (`crypto-channel`), halo2 routing circuit (`halo2`), and the `#[lumen_agent]` proc-macro (`macros`):

```bash
$ cargo test --workspace --features lumen-channel/crypto-channel,lumen-zkml/halo2,lumen-sdk/macros
```

To see the full behavior at a glance:

```bash
$ cargo run -p lumen-cli --example hello_agent
```

This demo walks through a 3-step sequence — echo, add, and jailbreak blocking — showing all [four security pillars](INTRODUCTION_EN.md#four-core-pillars) in action.

## CLI Usage

As of v0.4, the `lumen` binary provides 7 subcommands: `init`, `verify-model`, `sbom`, `defend`, `prove`, `run`, and `verifier`. `init` outputs a policy file template to stdout, so redirect it to a path such as `policies/my_policy.toml` to get started.

```bash
$ cargo run -p lumen-cli -- init > policies/my_policy.toml
```

`defend` performs prompt injection analysis on arbitrary text and outputs the `Verdict` and `corpus_version`.

```bash
$ cargo run -p lumen-cli -- defend --text "ignore previous instructions"
```

`verify-model` compares a manifest against an actual file and verifies the BLAKE3 hash and Ed25519 signature (if present).

```bash
$ cargo run -p lumen-cli -- verify-model --manifest model.toml --file model.safetensors
```

`sbom` outputs a CycloneDX 1.5 SBOM as JSON from the list of models declared in a policy file.

```bash
$ cargo run -p lumen-cli -- sbom --policy policies/default.toml
```

`prove` generates a mock ZKP for a tool routing decision and immediately self-verifies it. The circuit identifier, prompt hash, proof digest, and verification verdict are all printed to stdout.

```bash
$ cargo run -p lumen-cli -- prove --prompt "echo hi" --tool echo --circuit-id lumen.routing.v1
```

`run` executes one agent step while enforcing the BLAKE3 pin of the policy file. If `--policy-hash` does not match the actual BLAKE3 of the policy file, the request is immediately rejected — this is the core of Lumen's Zero-Trust UX. If a model manifest is declared in the policy file, the `verify_model` step must pass automatically; after passing, the host side performs capability issuance, defense analysis, mock inference, policy verification, tool execution, ZKP generation, and self-verify all in one shot.

```bash
$ cargo run -p lumen-cli -- run --policy policies/default.toml --policy-hash <BLAKE3HEX> --prompt "echo hello"
```

`verifier` is the on-chain verifier tool added in v0.4, with two subcommands: `emit` and `deploy`. `verifier emit` deterministically writes EVM (Solidity) or Mina (o1js) verifier source, a deploy script, and a metadata JSON to disk (byte-identical output guaranteed for the same input). `verifier deploy` executes `forge create` or `zk deploy` as a child process, but defaults to a dry run — only printing a redacted command string. The EVM private key is read from the `LUMEN_DEPLOY_PRIVKEY` environment variable and masked as `***` in audit logs to prevent leakage. For air-gapped operation, the recommended workflow is to carry the directory produced by `emit` as text and copy only the dry-run output to the production environment.

```bash
$ cargo run -p lumen-cli -- verifier emit --chain evm --circuit-id lumen.routing.binary.v1 --out ./out/evm
$ cargo run -p lumen-cli -- verifier emit --chain mina --out ./out/mina
$ cargo run -p lumen-cli -- verifier deploy --chain evm --rpc https://sepolia.example.org --out ./out/evm
```

## Workspace Structure

As of v0.4, Lumen is a Cargo workspace consisting of 15 library crates and 1 binary crate. Additionally, two wasm32-only demo agents are separated as an independent workspace under `agents/`.

```text
crates/
  lumen-core         ID, BLAKE3, Ed25519 wrapper, Error, Time
  lumen-fixed        Q16.16 and Q8.24 deterministic integer arithmetic (no_std)
  lumen-capability   Capability tokens, PolicyEngine, AgentMessage resource
  lumen-channel      InProc, AttestedChannel, AES-GCM/x25519 EncryptedChannel
  lumen-provenance   Safetensors, ONNX, GGUF header verification, SBOM, PinSet auto-rotation
  lumen-defense      Three-stage injection filter based on Aho-Corasick and RegexSet
  lumen-zkml         ProvingSystem trait, Mock, ezkl stub, halo2 circuit
  lumen-inference    InferenceEngine/StreamingEngine traits, verified model loader, quantization config; Dummy/CandleLlm(GGUF)/TEE channel backends
  lumen-sandbox      wasmtime determinism Config and capability-gated imports
  lumen-agent        Agent runtime (defense → infer → policy → tool → prove)
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

## Feature Flags

- `lumen-channel/crypto-channel`: Enables EncryptedChannel with AES-GCM, x25519, and BLAKE3 KDF
- `lumen-zkml/halo2`: Compiles the halo2 binary argmax circuit and PLONKish constraints
- `lumen-zkml/ezkl`: ezkl placeholder
- `lumen-inference/candle-llm`: GGUF quantized LLM inference (candle-transformers + HuggingFace tokenizer, including token-by-token streaming)
- `lumen-inference/llama-cpp`: llama.cpp backend interface stub (to be completed in v0.5, requires cmake build)
- `lumen-sdk/macros`: Re-export of the `#[lumen_agent]` proc-macro
- `lumen-sdk/alloc`: Exposes dynamic String/Vec helper functions

All features are disabled by default and are intentionally opt-in to maintain consistency as an _air-gapped self-contained_ build.

## Verification Gates

A mergeable change must pass all four of the following commands:

```bash
$ cargo build  --workspace --all-targets
$ cargo test   --workspace
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo fmt    --all -- --check
```

To verify with all v0.4 features as well, append `--features lumen-channel/crypto-channel,lumen-zkml/halo2,lumen-sdk/macros` to the second and third commands above.

## Current Limitations and Placeholders

`MockCommitmentProver` is a BLAKE3 commitment, not a ZKP (`Verification::CommitmentOnly` and `Verification::ZkVerified` are separated at the type level, making confusion impossible by construction).

`Halo2Prover` is a real PLONKish circuit, but as of v0.4 it verifies with MockProver instead of a succinct KZG backend — off-line verifiability is therefore absent and will be migrated to KZG in a follow-up.

The `ezkl` backend is a feature stub. `DummyEngine` only recognizes echo and add patterns. `CandleLlmEngine` (the `candle-llm` feature) loads GGUF quantized models and supports token-by-token streaming generation; all model files are subject to mandatory BLAKE3 + optional Ed25519 verification via `VerifiedModelLoader`. The llama.cpp backend (the `llama-cpp` feature) is an interface-only stub to be completed in next milestone.

The EVM Solidity contract in `lumen-onchain` performs constraint rechecking; an upgrade to succinct proof verification is planned to proceed concurrently with the halo2 KZG migration.

TEE attestation PCK chain and VCEK signature verification are implemented only up to the measurement-pin stage as of v0.4.

## License

Dual-licensed under Apache-2.0 OR MIT; users may freely choose either. See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.

## Contributing

Lumen is intended as a contribution to the AI and security open-source ecosystem. As a security-first project, the contribution guidelines are relatively strict. For details, refer to the contribution guidelines in [CONTRIBUTING.md](CONTRIBUTING.md).
