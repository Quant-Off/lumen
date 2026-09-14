//! 폐쇄형(Air-Gapped) 환경 친화적 최소 BPE 토크나이저.
//!
//! HuggingFace `tokenizers` 크레이트 의존을 끊기 위한 자체 구현입니다.
//! GGUF 토크나이저 메타데이터(`tokenizer.ggml.tokens` + `tokenizer.ggml.merges`)
//! 또는 GPT-2 호환 `vocab.json` + `merges.txt` 를 그대로 받아 사용할 수 있는
//! 바이트 수준(byte-level) BPE 인코더/디코더입니다.
//!
//! # 결정성
//!
//! 본 토크나이저의 출력은 동일 vocab/merges 입력에 대해 비트-동일한 결정론적
//! 결과를 보장합니다. 부동소수 / 해시 테이블 비결정성을 피하기 위해 모든 내부
//! 자료구조는 [`BTreeMap`] 으로 통일했습니다.
//!
//! # 알고리즘 복잡도
//!
//! 단순 $O(n^2)$ 머지 루프(가장 낮은 rank 의 인접 페어를 매 라운드 검색)
//! 입니다. 일반적으로 토크나이저 입력은 ≤ 8 KiB 이고 호출 빈도가 적어
//! 실제 latency 에는 무시할 만한 수준입니다. 더 큰 입력에 대해서는 우선순위
//! 큐 기반 최적화(`O(n log n)`) 가 가능하나 본 마이크로커널 빌드에서는
//! 필요성이 낮아 의도적으로 채택하지 않았습니다.
//!
//! # GGUF 호환
//!
//! GGUF 의 토크나이저 메타데이터를 그대로 매핑할 수 있도록 다음 규약을
//! 따릅니다:
//!
//! - vocab 은 `Vec<Vec<u8>>` 형태로 받으며, 인덱스가 곧 토큰 ID 입니다.
//! - merges 는 `Vec<(Vec<u8>, Vec<u8>)>` 형태로 받으며, 등록 순서가 곧
//!   머지 우선순위(낮을수록 먼저 적용) 입니다.
//! - 토큰 ID 는 [`TokenId`] (`u32`) 로 표현됩니다.
//!
//! 실제 GGUF 파일에서 메타데이터를 읽어내는 파서는 추후 `gguf` 모듈로
//! 분리됩니다 - 본 모듈은 순수 BPE 로직만 다룹니다.

use std::collections::BTreeMap;

use lumen_core::{Error, Result};

/// BPE 토큰 식별자.
pub type TokenId = u32;

/// 바이트 수준 BPE 토크나이저.
#[derive(Clone, Debug)]
pub struct BpeTokenizer {
    /// 토큰 ID -> 바이트 시퀀스.
    vocab_by_id: Vec<Vec<u8>>,
    /// 바이트 시퀀스 -> 토큰 ID (역색인).
    id_by_token: BTreeMap<Vec<u8>, TokenId>,
    /// 머지 규칙 `(left, right) -> rank` (낮을수록 우선).
    merge_ranks: BTreeMap<(Vec<u8>, Vec<u8>), u32>,
    /// 알 수 없는 토큰 fallback ID. `None` 이면 인코드 실패시 에러.
    unk_id: Option<TokenId>,
}

impl BpeTokenizer {
    /// vocab + merges 로부터 새 인스턴스를 만듭니다.
    ///
    /// # Errors
    /// vocab 크기가 `u32` 범위를 초과하거나, 중복 토큰이 발견되거나, merges
    /// 길이가 `u32` 범위를 초과하면 [`Error::Invalid`] 를 반환합니다.
    pub fn new(vocab: Vec<Vec<u8>>, merges: Vec<(Vec<u8>, Vec<u8>)>) -> Result<Self> {
        if vocab.len() > u32::MAX as usize {
            return Err(Error::Invalid(
                "tokenizer: vocab too large for u32 ids".into(),
            ));
        }
        let mut id_by_token = BTreeMap::new();
        for (idx, tok) in vocab.iter().enumerate() {
            if id_by_token.insert(tok.clone(), idx as TokenId).is_some() {
                return Err(Error::Invalid(format!(
                    "tokenizer: duplicate vocab token at id {idx}"
                )));
            }
        }
        let mut merge_ranks = BTreeMap::new();
        for (rank, pair) in merges.into_iter().enumerate() {
            if rank > u32::MAX as usize {
                return Err(Error::Invalid(
                    "tokenizer: too many merges for u32 rank".into(),
                ));
            }
            // 동일 페어가 중복 등록되어도 첫 등록이 가장 낮은 rank 이므로
            // 그것을 보존합니다.
            merge_ranks.entry(pair).or_insert(rank as u32);
        }
        Ok(Self {
            vocab_by_id: vocab,
            id_by_token,
            merge_ranks,
            unk_id: None,
        })
    }

    /// 알 수 없는 토큰을 만났을 때 사용할 fallback ID 를 설정합니다.
    pub fn with_unk_id(mut self, unk_id: TokenId) -> Self {
        self.unk_id = Some(unk_id);
        self
    }

    /// 단일 토큰 바이트를 ID 로 조회합니다.
    pub fn token_to_id(&self, token: &[u8]) -> Option<TokenId> {
        self.id_by_token.get(token).copied()
    }

    /// ID 를 토큰 바이트로 조회합니다.
    pub fn id_to_token(&self, id: TokenId) -> Option<&[u8]> {
        self.vocab_by_id.get(id as usize).map(Vec::as_slice)
    }

    /// vocab 크기.
    pub fn vocab_size(&self) -> usize {
        self.vocab_by_id.len()
    }

    /// 등록된 머지 규칙 수.
    pub fn merge_count(&self) -> usize {
        self.merge_ranks.len()
    }

    /// UTF-8 텍스트를 토큰 ID 시퀀스로 인코드합니다.
    pub fn encode(&self, text: &str) -> Result<Vec<TokenId>> {
        self.encode_bytes(text.as_bytes())
    }

    /// 임의의 바이트 시퀀스를 토큰 ID 시퀀스로 인코드합니다.
    ///
    /// # 알고리즘
    /// 1. 입력 바이트마다 한 글자 토큰으로 분해.
    /// 2. 인접 페어 중 가장 낮은 rank 의 머지를 1 회 적용.
    /// 3. 더 이상 적용 가능한 머지가 없을 때까지 반복.
    /// 4. 각 토큰 바이트를 vocab ID 로 변환.
    pub fn encode_bytes(&self, bytes: &[u8]) -> Result<Vec<TokenId>> {
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        let mut pieces: Vec<Vec<u8>> = bytes.iter().map(|b| vec![*b]).collect();

        loop {
            let mut best_rank: u32 = u32::MAX;
            let mut best_idx: Option<usize> = None;
            for i in 0..pieces.len().saturating_sub(1) {
                // 머지 규칙 검색은 페어 자체를 키로 사용합니다. BTreeMap
                // 키는 빌릴 수 없으므로 검색용 페어를 만듭니다 - 짧은
                // 토큰이라 재할당 비용은 무시 가능합니다.
                let pair = (pieces[i].clone(), pieces[i + 1].clone());
                if let Some(&rank) = self.merge_ranks.get(&pair) {
                    if rank < best_rank {
                        best_rank = rank;
                        best_idx = Some(i);
                    }
                }
            }
            let Some(i) = best_idx else { break };
            let mut merged = Vec::with_capacity(pieces[i].len() + pieces[i + 1].len());
            merged.extend_from_slice(&pieces[i]);
            merged.extend_from_slice(&pieces[i + 1]);
            pieces[i] = merged;
            pieces.remove(i + 1);
        }

        let mut ids = Vec::with_capacity(pieces.len());
        for piece in pieces {
            if let Some(id) = self.id_by_token.get(&piece) {
                ids.push(*id);
            } else if let Some(unk) = self.unk_id {
                ids.push(unk);
            } else {
                return Err(Error::Inference(format!(
                    "tokenizer: unknown token (len={}) and no unk_id set",
                    piece.len()
                )));
            }
        }
        Ok(ids)
    }

    /// 토큰 ID 시퀀스를 바이트로 디코드합니다.
    pub fn decode_bytes(&self, ids: &[TokenId]) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for &id in ids {
            let token = self.id_to_token(id).ok_or_else(|| {
                Error::Inference(format!("tokenizer: unknown id {id} for decode"))
            })?;
            out.extend_from_slice(token);
        }
        Ok(out)
    }

    /// 토큰 ID 시퀀스를 UTF-8 문자열로 디코드합니다. 비-UTF-8 시퀀스는
    /// lossy 변환됩니다.
    pub fn decode(&self, ids: &[TokenId]) -> Result<String> {
        let bytes = self.decode_bytes(ids)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    /// 기본: 모든 단일 바이트가 vocab 에 있고 머지가 없으면, 인코드는
    /// 입력 바이트마다 1 토큰을 생성합니다.
    #[test]
    fn no_merges_byte_per_token() {
        let vocab = (0u8..=255u8).map(|b| vec![b]).collect();
        let tk = BpeTokenizer::new(vocab, vec![]).unwrap();
        let ids = tk.encode("abc").unwrap();
        assert_eq!(ids, vec![b'a' as u32, b'b' as u32, b'c' as u32]);
        assert_eq!(tk.decode(&ids).unwrap(), "abc");
    }

    /// 우선순위 검증: rank 0 페어가 rank 1 페어보다 먼저 머지됩니다.
    #[test]
    fn merge_priority_lowest_rank_first() {
        // vocab: a b c ab abc
        let vocab = vec![b("a"), b("b"), b("c"), b("ab"), b("abc")];
        // merges: (a,b) -> rank 0, (ab,c) -> rank 1
        let merges = vec![(b("a"), b("b")), (b("ab"), b("c"))];
        let tk = BpeTokenizer::new(vocab, merges).unwrap();
        let ids = tk.encode("abc").unwrap();
        // (a,b) -> ab, (ab,c) -> abc → 단일 토큰 "abc" (id=4).
        assert_eq!(ids, vec![4]);
        assert_eq!(tk.decode(&ids).unwrap(), "abc");
    }

    /// 동일 입력 / 동일 vocab 에서 결정론적 출력.
    #[test]
    fn deterministic() {
        let vocab = vec![b("a"), b("b"), b("ab")];
        let merges = vec![(b("a"), b("b"))];
        let tk = BpeTokenizer::new(vocab, merges).unwrap();
        let a = tk.encode("ab").unwrap();
        let b = tk.encode("ab").unwrap();
        assert_eq!(a, b);
    }

    /// 알 수 없는 토큰: unk_id 미설정 시 에러.
    #[test]
    fn unknown_without_unk_errors() {
        let vocab = vec![b("a")]; // 'b' 미포함
        let tk = BpeTokenizer::new(vocab, vec![]).unwrap();
        assert!(tk.encode("b").is_err());
    }

    /// 알 수 없는 토큰: unk_id 설정 시 fallback.
    #[test]
    fn unknown_with_unk_uses_fallback() {
        let vocab = vec![b("a"), b("<unk>")];
        let tk = BpeTokenizer::new(vocab, vec![]).unwrap().with_unk_id(1);
        let ids = tk.encode("b").unwrap();
        assert_eq!(ids, vec![1]);
    }

    /// 빈 입력은 빈 출력.
    #[test]
    fn empty_input() {
        let tk = BpeTokenizer::new(vec![b("a")], vec![]).unwrap();
        assert!(tk.encode("").unwrap().is_empty());
        assert!(tk.decode(&[]).unwrap().is_empty());
    }

    /// 중복 vocab 토큰은 거부.
    #[test]
    fn duplicate_vocab_rejected() {
        let vocab = vec![b("a"), b("a")];
        assert!(BpeTokenizer::new(vocab, vec![]).is_err());
    }

    /// 디코드 시 알 수 없는 ID 는 에러.
    #[test]
    fn decode_unknown_id_errors() {
        let tk = BpeTokenizer::new(vec![b("a")], vec![]).unwrap();
        assert!(tk.decode(&[42]).is_err());
    }

    /// 바이트 수준: UTF-8 다바이트 문자도 동작.
    #[test]
    fn byte_level_handles_multibyte_utf8() {
        let mut vocab: Vec<Vec<u8>> = (0u8..=255u8).map(|b| vec![b]).collect();
        // "한" 의 UTF-8 = 0xED 0x95 0x9C (3 바이트).
        let han: Vec<u8> = vec![0xED, 0x95, 0x9C];
        vocab.push(han.clone());
        let merges = vec![(vec![0xED], vec![0x95]), (vec![0xED, 0x95], vec![0x9C])];
        let tk = BpeTokenizer::new(vocab, merges).unwrap();
        let ids = tk.encode("한").unwrap();
        // 머지 후 단일 토큰 (마지막 ID = 256).
        assert_eq!(ids, vec![256]);
        assert_eq!(tk.decode(&ids).unwrap(), "한");
    }

    /// 인코드/디코드 round-trip.
    #[test]
    fn roundtrip_arbitrary_bytes() {
        // 256 개 단일 바이트 vocab + 일부 머지 규칙.
        let mut vocab: Vec<Vec<u8>> = (0u8..=255u8).map(|b| vec![b]).collect();
        vocab.push(b("ab"));
        vocab.push(b("cd"));
        let merges = vec![(b("a"), b("b")), (b("c"), b("d"))];
        let tk = BpeTokenizer::new(vocab, merges).unwrap();
        let original = "abcd-abcd";
        let ids = tk.encode(original).unwrap();
        assert_eq!(tk.decode(&ids).unwrap(), original);
    }

    /// vocab/merge 메타데이터 조회.
    #[test]
    fn metadata_accessors() {
        let vocab = vec![b("a"), b("b"), b("ab")];
        let merges = vec![(b("a"), b("b"))];
        let tk = BpeTokenizer::new(vocab, merges).unwrap();
        assert_eq!(tk.vocab_size(), 3);
        assert_eq!(tk.merge_count(), 1);
        assert_eq!(tk.token_to_id(b"ab"), Some(2));
        assert_eq!(tk.id_to_token(2), Some(&b"ab"[..]));
        assert_eq!(tk.id_to_token(99), None);
    }
}
