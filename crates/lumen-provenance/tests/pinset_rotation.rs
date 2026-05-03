//! End-to-end pinset rotation against on-disk safetensors files.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use lumen_core::{Blake3Hash, Timestamp};
use lumen_provenance::manifest::{Format, ModelManifest};
use lumen_provenance::{verify_model_against_pinset, PinAcceptance, PinEntry, PinSet};
use safetensors::tensor::TensorView;
use safetensors::Dtype;

fn write_tiny_safetensors(dir: &tempfile::TempDir, payload: &[u8], file: &str) -> PathBuf {
    let view = TensorView::new(Dtype::U8, vec![payload.len()], payload).unwrap();
    let mut tensors = HashMap::new();
    tensors.insert("t0".to_string(), view);
    let bytes = safetensors::serialize(&tensors, &None).unwrap();
    let path = dir.path().join(file);
    fs::write(&path, &bytes).unwrap();
    path
}

#[test]
fn current_pin_passes_against_real_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_tiny_safetensors(&dir, &[1, 2, 3, 4], "v1.safetensors");
    let hash = Blake3Hash::of_file(&path).unwrap();

    let manifest = ModelManifest {
        name: "tiny".into(),
        version: "v1".into(),
        path: path.clone(),
        format: Format::Safetensors,
        hash: Blake3Hash::of(b"unused-by-pinset"),
        license: None,
        signature: None,
        signer: None,
    };

    let pinset = PinSet::single(PinEntry {
        hash,
        version: Some("v1".into()),
    });
    let (info, acceptance) =
        verify_model_against_pinset(&path, &manifest, &pinset, Timestamp::from_millis(0), &[])
            .unwrap();
    assert_eq!(info.hash, hash);
    assert_eq!(acceptance, PinAcceptance::Current);
}

#[test]
fn rotated_old_file_accepted_during_grace() {
    let dir = tempfile::tempdir().unwrap();
    let v1 = write_tiny_safetensors(&dir, &[1, 2, 3, 4], "v1.safetensors");
    let v2 = write_tiny_safetensors(&dir, &[9, 9, 9, 9], "v2.safetensors");
    let h1 = Blake3Hash::of_file(&v1).unwrap();
    let h2 = Blake3Hash::of_file(&v2).unwrap();

    let mut pinset = PinSet::single(PinEntry {
        hash: h1,
        version: Some("v1".into()),
    });
    pinset.rotate(
        PinEntry {
            hash: h2,
            version: Some("v2".into()),
        },
        Timestamp::from_millis(1_000),
        60_000,
    );

    let manifest = ModelManifest {
        name: "tiny".into(),
        version: "any".into(),
        path: v1.clone(),
        format: Format::Safetensors,
        hash: Blake3Hash::of(b"unused"),
        license: None,
        signature: None,
        signer: None,
    };

    // grace 안에서 v1 도 통과.
    let (_, acc) =
        verify_model_against_pinset(&v1, &manifest, &pinset, Timestamp::from_millis(2_000), &[])
            .unwrap();
    match acc {
        PinAcceptance::Grace { .. } => {}
        other => panic!("expected Grace, got {other:?}"),
    }

    // grace 이후 v1 은 거부.
    let err =
        verify_model_against_pinset(&v1, &manifest, &pinset, Timestamp::from_millis(70_000), &[])
            .unwrap_err();
    assert!(err.to_string().contains("pinset rejected"), "got: {err}");
}

#[test]
fn unknown_file_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let v1 = write_tiny_safetensors(&dir, &[1, 2, 3, 4], "v1.safetensors");
    let other = write_tiny_safetensors(&dir, &[42; 16], "other.safetensors");

    let h1 = Blake3Hash::of_file(&v1).unwrap();
    let pinset = PinSet::single(PinEntry {
        hash: h1,
        version: None,
    });

    let manifest = ModelManifest {
        name: "tiny".into(),
        version: "v1".into(),
        path: v1.clone(),
        format: Format::Safetensors,
        hash: Blake3Hash::of(b"unused"),
        license: None,
        signature: None,
        signer: None,
    };

    let err =
        verify_model_against_pinset(&other, &manifest, &pinset, Timestamp::from_millis(0), &[])
            .unwrap_err();
    assert!(err.to_string().contains("pinset rejected"));
}
