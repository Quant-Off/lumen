---
analysis_date: 2026-05-07
focus: concerns
last_mapped_commit: dfba311
---

# Codebase Concerns

**Analysis Date:** 2026-05-07

## Air-Gap Migration Debt (v0.4→v0.5)

### Stale Documentation (PRACTICE_LLM.md / PRACTICE_LLM_EN.md)

**Issue:** Documentation still references removed feature `candle-llm` and deleted type `CandleLlmEngine`.

**Files affected:**
- `PRACTICE_LLM.md` (~660 lines) — lines 31–41, 141, 171, 277–281, 341, 356, 419, 503–507, 564, 590, 614 mention `candle-llm` feature, `CandleLlmEngine::from_gguf()`, and GGUF-based text generation workflow
- `PRACTICE_LLM_EN.md` (~660 lines) — English translation with identical references

**Impact:** Users following these guides will encounter broken imports and missing types. Builds with `features = ["candle-llm"]` will fail.

**Root cause:** kernel-comp.md (lines 99–106) documents removal of `candle-transformers` + `tokenizers` for air-gap compliance, but docs were not updated. Feature gate was removed but documentation remains.

**Fix approach:**
1. Remove or relocate PRACTICE_LLM*.md to a `docs/deprecated/` directory with a deprecation notice.
2. Create new guide `PRACTICE_ONNX.md` for `CandleEngine` (ONNX routing only).
3. Create future `PRACTICE_LLAMA_CPP.md` once `llama-cpp-2` backend is implemented (v0.5).

**Priority:** High — blocks users from following written examples.

---

### Stub Inference Backends (Not Yet Implemented)

#### LlamaCppEngine — v0.5 milestone

**Issue:** `lumen-inference/src/llama_cpp.rs` is a stub returning `Error::NotImplemented`.

**Files:**
- `crates/lumen-inference/src/llama_cpp.rs:54–56` — `InferenceEngine::complete()` returns error
- `crates/lumen-inference/src/llama_cpp.rs:67–69` — `StreamingEngine::stream_complete()` returns error

**Symptoms:**
- Code compiles with `--features llama-cpp` but runtime fails with "llama-cpp-2 연동이 v0.5 에서 구현됩니다"
- No actual llama.cpp bindings present

**Workaround:** Disable `llama-cpp` feature; use `CandleEngine` (ONNX routing) until v0.5.

**Fix approach:** Implement `llama-cpp-2` crate bindings per kernel-comp.md lines 272–276.

**Priority:** Medium — v0.5 milestone task, documented.

---

#### TeeChannel (Host-TEE Secure Channel) — v0.3 stub

**Issue:** `lumen-channel/src/tee.rs` implements `SecureChannel` trait but returns `Error::NotImplemented` for all operations.

**Files:**
- `crates/lumen-channel/src/tee.rs:36` — `send_bytes()` stub
- `crates/lumen-channel/src/tee.rs:40` — `recv_bytes()` stub

**Symptoms:**
- Any agent step using GPU inference via TEE channel will fail
- Current inference pipeline cannot forward GPU inference to hardware-accelerated TEE

**Impact:** No E2E Host-WASM-TEE integration. LLM inference runs only in WASM sandbox (CPU only) or via `ChannelEngine` forward (not yet wired).

**Workaround:** Use `CandleEngine` (CPU-bound ONNX routing) for tool selection; do not attempt TEE inference yet.

**Fix approach:**
1. Implement attested TLS-like handshake over postcard frames (kernel-comp.md lines 5–6 envision attested handshake, but not yet implemented).
2. Wire `lumen-attestation` TDX quote validation into handshake (see `/crates/lumen-attestation/src/tdx.rs:19–25` — PCK chain validation deferred to v0.4).
3. Define `SecureChannel` framing protocol.

**Priority:** Medium-High — blocks TEE + WASM split-execution model, a core security goal.

---

#### EzklProver (ZK Backend) — v0.3 stub

**Issue:** `lumen-zkml/src/ezkl.rs` is a feature-gated stub without real ezkl implementation.

**Files:**
- `crates/lumen-zkml/src/ezkl.rs:31` — `setup()` returns `Error::NotImplemented("ezkl::setup")`
- `crates/lumen-zkml/src/ezkl.rs:39` — `prove()` stub
- `crates/lumen-zkml/src/ezkl.rs:47` — `verify()` stub

**Symptoms:**
- `--features ezkl` compiles but calls fail at runtime
- No actual zero-knowledge proofs; only mock (BLAKE3 commitment) verification available

**Impact:** ZK proof path is not wired. Agent routing decisions are *not* zero-knowledge; mock prover only commits to witness (CommitmentOnly verification). Per Cargo.toml line 100–102, halo2 was removed and no succinct ZK backend replaces it.

**Current state:** Mock prover uses BLAKE3-based commitment (cheap, deterministic) instead of actual ZK. Per `lumen-agent/src/runtime.rs:31–32`, verification is always `CommitmentOnly` or `Invalid`, never `ZkVerified`.

**Fix approach:** kernel-comp.md (line 246) defers to SP1 / RISC Zero selection at v0.4 milestone review. No action needed until then.

**Priority:** Low — deferred by design; mock prover sufficient for audit trail.

---

## External System Dependencies (Require Vendor Bundling)

### Candle Build Dependency: Protocol Buffers (protoc)

**Issue:** `candle-core` feature requires `protoc` (Protocol Buffers compiler) at build time.

**Files:** `Cargo.toml:96` — `candle-core = { version = "0.10", default-features = false }`

**Symptoms:**
- Building with `--features candle` fails if `protoc` is not in `$PATH`
- Air-gap environment: no network to download protoc; must be pre-bundled

**Workaround:** Pre-install `protoc` (via `apt`, `brew`, or direct download) before building in air-gap.

**Fix approach:**
- kernel-comp.md (line 277) recommends source vendoring. Add vendoring instructions to build docs.
- Consider alternative: `llama-cpp-2` backend does not require protoc (only cmake + llama.cpp C++ build).

**Priority:** Medium — affects reproducible air-gap builds; documented in kernel-comp.md but not yet automated.

---

### Llama.cpp Build Dependency: cmake + llama.cpp library

**Issue:** Future `llama-cpp-2` feature will require cmake and precompiled llama.cpp library.

**Files:** `crates/lumen-inference/src/llama_cpp.rs` (stub; no deps yet)

**Current state:** Not yet a blocker (stub stage), but will be in v0.5.

**Workaround:** Pre-build llama.cpp library for target OS/architecture.

**Priority:** Low — future concern (v0.5 milestone).

---

## In-House Cryptography Dependencies (Non-Portable)

### Workspace Path Dependencies: elib-k0-nt Modules

**Issue:** Workspace imports cryptographic functions from sibling `../elib-k0-nt/*` path dependencies.

**Files:**
- `Cargo.toml:55–61` — workspace dependencies declared:
  - `elib-blake` (BLAKE3 KDF + hashing)
  - `elib-ed25519` (Ed25519 signing/verification)
  - `elib-x25519` (X25519 ECDH)
  - `elib-aes` (AES-256-GCM)
  - `elib-rng` (Hash-DRBG-SHA256)
  - `elib-constant-time` (constant-time comparison)
  - `elib-zeroize` (memory zeroization)

**Impact:**
- **Downstream consumers cannot use Lumen as a library** — crates importing Lumen must also have `../elib-k0-nt/` in their workspace.
- Not published on crates.io (path-only).
- Violates Rust crate ecosystem conventions.

**Root cause:** Intentional per kernel-comp.md (air-gap strategy). elib-k0-nt replaces standard crates (blake3, ed25519-dalek, x25519-dalek, aes-gcm, rand) for FIPS 140-3 compliance path.

**Fix approach (v1.0 milestone):**
1. Publish elib-k0-nt modules to crates.io with stable semver.
2. Switch path dependencies to `crates.io` versions with version bounds.
3. Implement FIPS 140-3 audit trail (currently deferred per CLAUDE.md line 28).

**Priority:** Low — accepted trade-off for air-gap compliance; v1.0 concern.

---

## Determinism Risk: Floating-Point in Candle Path

### F32 Inference Non-Determinism

**Issue:** `CandleEngine` uses f32 operations which may differ across hardware/compiler.

**Files:**
- `crates/lumen-inference/src/candle.rs:6` — documents f32 concern
- `crates/lumen-inference/src/candle.rs:12` — "candle의 f32 연산은 미세하게 비결정적일 수 있으므로"
- `crates/lumen-inference/src/candle.rs:66–77` — `encode_prompt()` converts BLAKE3 hash to f32 vector
- `crates/lumen-inference/src/candle.rs:99–104` — inference output to f32, then argmax

**Impact:**
- ZK proofs are NOT bound to Candle's floating-point outputs (by design).
- Per runtime.rs:16, proofs bind only to "integer routing index" (argmax result), not float values.
- This breaks reproducibility if Candle's f32 argmax differs between runs.

**Current mitigation:**
- `lumen-fixed/src/q16_16.rs` (233 lines) implements fixed-point arithmetic.
- Per candle.rs:14–17, ZK witnesses use integer decision only.

**Residual risk:** If two runs produce different argmax due to f32 rounding, the tool routing changes without proof notification. No automated test yet checks cross-platform f32 determinism.

**Fix approach:**
- Add CI test comparing Candle f32 outputs across x86-64 and ARM64.
- Migrate Candle inference to fixed-point engine (`lumen-fixed`) for tooling (v0.5 milestone).

**Priority:** Medium — affects audit trail integrity; partially mitigated by integer binding.

---

## Missing TEE Attestation Chain Integration

### Incomplete Policy Enforcement + TEE Attestation Wiring

**Issue:** `lumen-agent/src/runtime.rs` does not enforce TEE attestation policy in agent step.

**Files:**
- `crates/lumen-agent/src/runtime.rs:98–112` — `step()` orchestrates defense → inference → policy → tool → prove, but no attestation checkpoint
- `crates/lumen-attestation/src/tdx.rs:19–25` — TDX Quote v4 parser exists, but X.509 chain + signature verification deferred to v0.4
- `crates/lumen-channel/src/tee.rs:1–7` — TeeChannel stub; no actual attested handshake

**Symptoms:**
- TEE hardware attestation is parsed but not enforced.
- No check that GPU inference host is inside a TEE with valid quote.
- Agent step proceeds even if TEE attestation fails.

**Impact:** E2E zero-trust guarantee is incomplete. WASM sandbox is isolated, but GPU inference (TEE) can silently fail attestation and agent does not know.

**Fix approach:**
1. Implement X.509 PCK chain verification in TDX parser (kernel-comp.md lines 25–26 flag as v0.4 work).
2. Wire quote validation into `TeeChannel::new()` handshake.
3. Fail agent step if TEE attestation invalid.

**Priority:** High — core security property (zero-trust) is not fully enforced.

---

## Secret Scanning & SBOM Automation

### No Automated Secret Detection

**Issue:** No pre-commit secret scanner (e.g., git-secrets, truffleHog) in CI/CD.

**Risk:** Credentials (AWS keys, HF tokens, PCS API keys) may be committed to repo.

**Current state:** Manual review only. No automated detection.

**Fix approach:**
- Add `git-secrets` or `truffleHog` to CI pipeline.
- Run `cargo deny` for crate advisories (already done per commit eed4194).

**Priority:** Medium — security hygiene.

---

### SBOM Generation Not Automated

**Issue:** CLAUDE.md line 20 lists "SBOM를 강제 생성" but no automation present.

**Files:** Cargo.toml (workspace), no `cyclonedx` or `cargo-sbom` integration

**Current state:** Manual `cargo tree` only.

**Fix approach:**
- Add `cargo cyclonedx` or `cargo-sbom` to CI.
- Generate CycloneDX / SPDX SBOM on every release.

**Priority:** Medium — compliance (v1.0 milestone goal).

---

## Performance Concerns

### WASM Sandbox Cold-Start Not Benchmarked

**Issue:** No benchmark for wasmtime instance creation + compilation time.

**Files:** No benchmark files found (`crates/*/benches/` empty)

**Symptoms:**
- Unknown latency for agent step startup.
- Unclear if fuel-based epoch enforcement adds overhead.
- Can't track regression.

**Current state:** DummyEngine used in tests (instant completion); no real inference + WASM overhead measured.

**Fix approach:**
- Add criterion benchmark: `Agent step (end-to-end, ONNX routing via Candle)`.
- Measure wasmtime instantiation overhead separately.
- Set performance gates in CI.

**Priority:** Medium — needed for production SLA planning.

---

### Fixed-Point Pipeline Not Enforced

**Issue:** `lumen-fixed/src/q16_16.rs` exists but not integrated into Candle inference path.

**Files:** `crates/lumen-fixed/src/q16_16.rs` (233 lines) — standalone fixed-point type, no usage in lumen-inference or lumen-agent

**Impact:** Floating-point non-determinism risk (above) persists because fixed-point is not applied.

**Current state:** Placeholder for future quantization path.

**Fix approach:** Integrate fixed-point routing into CandleEngine (v0.5 work).

**Priority:** Medium — depends on llama-cpp and determinism testing.

---

## Workspace Feature Fragmentation

### Candle Feature Scattered Across Codebase

**Issue:** `candle` feature activates candle-core + candle-onnx, but feature state unclear in dependent crates.

**Files:**
- `Cargo.toml:96–97` — workspace-level optional deps
- `crates/lumen-inference/Cargo.toml:21` — `candle` feature gate

**Risk:** Accidental feature activation in transitive dependencies. No enforcement that downstream crates properly gate Candle code.

**Fix approach:**
- Document feature hygiene in CONTRIBUTING.md.
- Add CI check: `cargo build --no-default-features --features candle` to verify isolated build.

**Priority:** Low — feature flags are documented; low risk.

---

## Code Hygiene Issues

### Dead Code Markers Without Cleanup

**Issue:** `#[allow(dead_code)]` on struct fields in stub implementations.

**Files:**
- `crates/lumen-inference/src/llama_cpp.rs:28–34` — three `#[allow(dead_code)]` on LlamaCppEngine fields
- `crates/lumen-capability/src/policy.rs:134` — `#[allow(dead_code)]` on policy field

**Symptoms:** Suppresses legitimate warnings during stub phase, but masks real unused code when stubs are removed.

**Fix approach:**
- Remove `#[allow]` when stubs are implemented.
- Use `#[cfg(feature = "...")]` instead of `#[allow]` for conditional code.

**Priority:** Low — cleanup task; low risk.

---

### Defense Pattern Library Needs Tuning

**Issue:** `lumen-defense/src/lexicon.rs:11` flags TODO.

**Files:** `crates/lumen-defense/src/lexicon.rs:11–46`

**Text:**
```
TODO: 구글링 해보니 이 부분에 대해 토큰 사용량과 적절한 패턴의 밸런스를
중시해야 한다고 하네요. 검토 필요 있음.
```

**Issue:** Jailbreak lexicon is conservative (46 patterns) but may need expansion or refinement based on token budget vs false-positive rate.

**Current patterns:** "ignore previous instructions", "dan mode", "developer mode", etc.

**Impact:** May miss novel jailbreak attempts; false positives block legitimate prompts.

**Fix approach:**
- Add tuning parameter (toxicity threshold) to `LexiconEngine`.
- Expand pattern set post-deployment based on telemetry.
- Implement allowlist for safe keywords (e.g., "developer" in code context).

**Priority:** Low — current lexicon is conservative (safe); tuning is a v0.5+ task.

---

## Dependency Monitoring

### Wasmtime Security Advisory Backlog

**Issue:** `Cargo.toml:75` pins wasmtime v44, which has had multiple security fixes.

**Current version:** v44 (comment: "최신 line. RUSTSEC 다수 advisory (fd_renumber / wasi:http fields / ...)")

**Risk:**
- New advisories may emerge after v44 release.
- No automated tracking (dependabot not visible in config).

**Fix approach:**
- Enable GitHub Dependabot for Cargo.lock updates.
- Monitor RUSTSEC for wasmtime advisories.
- Establish upgrade cadence (quarterly wasmtime bump).

**Priority:** Medium — security-critical dependency.

---

## Documentation Coverage

### Architectural Decision Records (ADRs) Missing

**Issue:** No ADR for kernel-comp.md migration decisions.

**Files:** `kernel-comp.md` (15K lines) documents air-gap migration but is not indexed in README or ARCHITECTURE docs.

**Impact:** Future maintainers may not understand why elib-k0-nt is used, why halo2 was removed, etc.

**Fix approach:**
- Extract kernel-comp.md decisions into `.adr/` directory as individual ADRs.
- Link from README.md and CLAUDE.md.

**Priority:** Low — knowledge preservation; no code impact.

---

## Summary by Risk Level

| Issue | Severity | Category | Status |
|-------|----------|----------|--------|
| PRACTICE_LLM*.md stale docs | High | Air-gap debt | Immediate |
| Missing TEE attestation enforcement | High | Security | High priority |
| LlamaCppEngine stub (v0.5) | Medium | Feature milestone | Expected |
| TeeChannel stub (v0.3) | Medium-High | Integration | Blocks TEE model |
| Candle f32 non-determinism | Medium | Audit integrity | Mitigated by design |
| No secret scanning | Medium | CI/CD hygiene | Preventive |
| SBOM automation missing | Medium | Compliance (v1.0) | Deferred |
| Wasmtime advisory backlog | Medium | Dependency | Monitoring |
| WASM cold-start benchmark | Medium | Perf | Nice-to-have |
| Candle feature fragmentation | Low | Code hygiene | Low risk |
| Dead code markers | Low | Cleanup | Low risk |
| Defense lexicon tuning | Low | Content | v0.5+ |
| ADR documentation | Low | Preservation | Nice-to-have |

---

*Concerns audit: 2026-05-07*
