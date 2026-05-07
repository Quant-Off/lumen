//! End-to-end verification of safetensors files against a manifest.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use lumen_core::Blake3Hash;
use lumen_provenance::manifest::{Format, ModelManifest};
use lumen_provenance::verify_model;
use safetensors::tensor::TensorView;
use safetensors::Dtype;

fn write_tiny_safetensors(dir: &tempfile::TempDir) -> PathBuf {
    let data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let view = TensorView::new(Dtype::U8, vec![data.len()], &data).unwrap();
    let mut tensors = HashMap::new();
    tensors.insert("t0".to_string(), view);
    let bytes = safetensors::serialize(&tensors, None).unwrap();
    let path = dir.path().join("tiny.safetensors");
    fs::write(&path, &bytes).unwrap();
    path
}

#[test]
fn verify_clean_file_passes() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_tiny_safetensors(&dir);
    let hash = Blake3Hash::of_file(&path).unwrap();
    let manifest = ModelManifest {
        name: "tiny".into(),
        version: "0.0.1".into(),
        path: path.clone(),
        format: Format::Safetensors,
        hash,
        license: Some("Apache-2.0".into()),
        signature: None,
        signer: None,
    };
    let info = verify_model(&path, &manifest, &[]).unwrap();
    assert_eq!(info.name, "tiny");
}

#[test]
fn verify_tampered_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_tiny_safetensors(&dir);
    let hash = Blake3Hash::of_file(&path).unwrap();
    let manifest = ModelManifest {
        name: "tiny".into(),
        version: "0.0.1".into(),
        path: path.clone(),
        format: Format::Safetensors,
        hash,
        license: None,
        signature: None,
        signer: None,
    };
    // Flip one byte at the end (in tensor data, not header).
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&path, &bytes).unwrap();

    let err = verify_model(&path, &manifest, &[]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("hash mismatch"), "got: {msg}");
}

#[test]
fn verify_with_signature() {
    use lumen_core::rng::OsRng;
    use lumen_core::SigningKey;

    let dir = tempfile::tempdir().unwrap();
    let path = write_tiny_safetensors(&dir);
    let hash = Blake3Hash::of_file(&path).unwrap();
    let sk = SigningKey::generate(&mut OsRng);
    let mut manifest = ModelManifest {
        name: "tiny".into(),
        version: "0.0.1".into(),
        path: path.clone(),
        format: Format::Safetensors,
        hash,
        license: Some("MIT".into()),
        signature: None,
        signer: None,
    };
    manifest.sign_with(&sk).unwrap();

    let info = verify_model(&path, &manifest, &[sk.verifying_key()]).unwrap();
    assert_eq!(info.size_bytes, fs::metadata(&path).unwrap().len());

    // Untrusted signer ⇒ rejected.
    let other = SigningKey::generate(&mut OsRng);
    let err = verify_model(&path, &manifest, &[other.verifying_key()]).unwrap_err();
    assert!(err.to_string().contains("signature"));
}

/// 회귀: trusted_signers 가 비어있지 않은데 매니페스트가 미서명이면 거부.
/// 이전 구현은 (signature, signer) 가 None 이면 silently skip 했음.
#[test]
fn unsigned_manifest_rejected_when_signers_required() {
    use lumen_core::rng::OsRng;
    use lumen_core::SigningKey;

    let dir = tempfile::tempdir().unwrap();
    let path = write_tiny_safetensors(&dir);
    let hash = Blake3Hash::of_file(&path).unwrap();
    let trusted = SigningKey::generate(&mut OsRng);

    // signature/signer 가 None 이지만 trusted_signers 는 비어있지 않음.
    let manifest = ModelManifest {
        name: "tiny".into(),
        version: "0.0.1".into(),
        path: path.clone(),
        format: Format::Safetensors,
        hash,
        license: None,
        signature: None,
        signer: None,
    };
    let err = verify_model(&path, &manifest, &[trusted.verifying_key()]).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unsigned"),
        "expected unsigned-rejection, got: {msg}"
    );
}
