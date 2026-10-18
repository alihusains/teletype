//! Pure-Rust SentencePiece BPE tokenizer for the NeuDecide model.
//!
//! Faithful port (verified against the Python `sentencepiece` wheel that
//! `neudecide` runs on, and against the C++ source in the sentencepiece
//! 0.2.1 sdist) of the exact path the `neudecide` Python package uses to
//! tokenize tool lists and detokenize model output.
//!
//! The HF `tokenizer.json` that ships in the model directory is
//! information-lossy: all BPE merge scores are zero there, which makes the
//! `tokenizers`-crate BPE merge in the wrong order. The authoritative file is
//! the binary `tokenizer.model` (proto2 `ModelProto`), which this module
//! parses directly (hand-rolled wire decoding, no protobuf dependency).
//!
//! # Encoding (normalized text -> ids), mirroring
//! `SentencePieceProcessor::Encode` for a BPE model with the `identity`
//! normalizer (empty charmap) and `byte_fallback = true`:
//!
//! 1. Strip leading ASCII spaces. If nothing remains, return `[]`.
//! 2. Prepend the escape marker `▁` (U+2581), then walk the text one
//!    character at a time: a space or run of spaces becomes one `▁` (the run
//!    collapses); every other character passes through unchanged.
//! 3. Trim trailing `▁` (the text ended in spaces).
//! 4. Split into symbols: at each position, if a user-defined piece
//!    (`<tool_call>` or `<tools>`, read from the model's `USER_DEFINED`
//!    pieces, not the trainer spec) starts there, consume the whole piece as
//!    a *frozen* symbol (it never merges); otherwise consume one Unicode
//!    character.
//! 5. Merge: a binary max-heap over adjacent non-frozen symbol pairs, keyed
//!    by the score of the concatenated piece in the vocab (higher first,
//!    leftmost on ties). A pair is merged into the left symbol; the right
//!    symbol is consumed. Stale heap entries are skipped when either
//!    symbol's text no longer matches the recorded size.
//! 6. Final id per surviving symbol: reserved pieces (control / unknown /
//!    byte) first, then the normal vocab; an unmatched symbol is necessarily
//!    a single unmatched character and is decomposed into its UTF-8 byte
//!    pieces (`<0xXX>`), because `byte_fallback` is true for this model.
//!
//! # Decoding (ids -> text), mirroring `SentencePieceProcessor::Decode`:
//!
//! - An out-of-range id is an error (the Python binding raises
//!   `IndexError`).
//! - Control pieces (`<pad>`, `</s>`, `<s>`) decode to nothing, and do not
//!   consume the leading-whitespace position.
//! - `<unk>` decodes to the model's `unk_surface` (trainer spec field 44;
//!   when absent in the file, the C++ `kDefaultUnknownSymbol`, `" \u{2047} "`).
//! - Byte pieces are buffered; a maximal run of byte pieces is reassembled
//!   into Unicode characters, each byte that is not part of a valid UTF-8
//!   character becoming `U+FFFD`.
//! - Text pieces: the *first* piece while the leading-whitespace position is
//!   still open drops one leading `▁` (the dummy-prefix marker); every other
//!   `▁` becomes a space.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{DeciderError, Result};

/// Raw piece kinds, indexed by id, for the decode path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PieceKind {
    /// A normal / user-defined / unused piece; text is stored in `text`.
    Text,
    /// A `<0xXX>` byte piece; the byte is stored in `byte_by_id`.
    Byte,
    /// `<pad>`, `</s>`, `<s>`: decode to nothing.
    Control,
    /// `<unk>`: decodes to `unk_surface`.
    Unk,
}

/// A SentencePiece BPE tokenizer loaded from a binary `tokenizer.model`.
///
/// Owned by the ONNX runner; `encode`/`decode` are cheap (the vocab is a
/// hash map) and take no locks of their own.
#[derive(Debug)]
pub struct SpmTokenizer {
    /// Piece text -> id, for every piece in the model.
    id_of: HashMap<String, u32>,
    /// Concatenated-piece text -> merge score, for mergeable pieces only
    /// (NORMAL | USER_DEFINED | UNUSED). The C++ `pieces_` map; control and
    /// byte pieces are *not* merge candidates even though their texts are
    /// in the vocab.
    mergeable: HashMap<String, f32>,
    /// Decode table indexed by id.
    kind: Vec<PieceKind>,
    /// Text of the piece, by id (only for [`PieceKind::Text`]).
    text: Vec<String>,
    /// Byte value of the piece, by id (only for [`PieceKind::Byte`]).
    byte_by_id: Vec<u8>,
    /// Byte value -> id of the matching `<0xXX>` piece.
    byte_id: [u32; 256],
    /// User-defined piece texts (model pieces of type `USER_DEFINED`),
    /// longest first (for the frozen prefix match).
    user: Vec<String>,
    /// Surface text of the unknown piece, from `TrainerSpec.unk_surface`
    /// (field 44; when absent in the file, the C++ default `" \u{2047} "`).
    unk_surface: String,
}

impl SpmTokenizer {
    /// Load and validate a `tokenizer.model`.
    ///
    /// Fails (rather than degrading) when the model is not the known
    /// BPE + identity-normalizer configuration the NeuDecide model ships:
    /// a wrong-shape model would tokenize silently wrong.
    pub fn load(path: &Path) -> std::result::Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut p = Proto::new(&data);

        let mut pieces: Vec<(String, f32, u32)> = Vec::new();
        let mut model_type: u32 = 0;
        let mut byte_fallback: u32 = 0;
        let mut unk_surface_raw: Option<Vec<u8>> = None;
        let mut norm_name: Option<String> = None;
        let mut norm_charsmap_len: usize = 0;
        let mut norm_remove_extra: Option<u32> = None;

        p.each_field(&[1, 2, 3], |num, _wire, v| match num {
            1 => {
                // SentencePiece { 1 = string piece, 2 = float score,
                //                 3 = varint type }
                let mut piece = String::new();
                let mut score: f32 = 0.0;
                let mut typ: u32 = 1;
                Proto::new(v)
                    .each_field(&[1, 2, 3], |ff, w, vv| {
                        match (ff, w) {
                            (1, 2) => piece = as_string(vv),
                            (2, 5) => score = as_f32(vv),
                            (3, 0) => typ = as_varint(vv) as u32,
                            _ => {}
                        }
                        Ok(())
                    })
                    .map_err(|e| format!("piece: {e}"))?;
                pieces.push((piece, score, typ));
                Ok(())
            }
            2 => {
                // TrainerSpec { 3 = model_type, 35 = byte_fallback,
                //               44 = unk_surface }
                Proto::new(v)
                    .each_field(&[3, 35, 44], |ff, w, vv| {
                        match (ff, w) {
                            (3, 0) => model_type = as_varint(vv) as u32,
                            (35, 0) => byte_fallback = as_varint(vv) as u32,
                            (44, 2) => unk_surface_raw = Some(vv.to_vec()),
                            _ => {}
                        }
                        Ok(())
                    })
                    .map_err(|e| format!("trainer: {e}"))?;
                Ok(())
            }
            3 => {
                // NormalizerSpec { 1 = name, 2 = precompiled_charsmap,
                //                  4 = remove_extra_whitespaces }
                Proto::new(v)
                    .each_field(&[1, 2, 4], |ff, w, vv| {
                        match (ff, w) {
                            (1, 2) => norm_name = Some(as_string(vv)),
                            (2, 2) => norm_charsmap_len = vv.len(),
                            (4, 0) => norm_remove_extra = Some(as_varint(vv) as u32),
                            _ => {}
                        }
                        Ok(())
                    })
                    .map_err(|e| format!("normalizer: {e}"))?;
                Ok(())
            }
            _ => Ok(()),
        })
        .map_err(|e| format!("parse tokenizer.model: {e}"))?;

        // Strict validation: this tokenizer only supports the known model
        // shape. Anything else is a wrong/updated model file.
        let err = |m: &str| -> std::result::Result<Self, String> {
            Err(format!("unsupported tokenizer.model {path:?}: {m}"))
        };
        if let Some(pos) = pieces.iter().position(|(p, _, _)| p.is_empty()) {
            return Err(format!(
                "empty piece at index {pos}: breaks the frozen-UDS match and BPE merge"
            ));
        }
        if model_type != 2 {
            return err("model_type != BPE");
        }
        if byte_fallback != 1 {
            return err("byte_fallback must be true");
        }
        let unk_count = pieces.iter().filter(|(_, _, t)| *t == 2).count();
        if unk_count != 1 {
            return err("expected exactly one <unk> piece");
        }
        if norm_name.as_deref() != Some("identity") {
            return err("normalizer must be 'identity'");
        }
        if norm_charsmap_len != 0 {
            return err("normalizer charsmap must be empty");
        }
        if norm_remove_extra == Some(0) {
            return err("remove_extra_whitespaces must be true");
        }
        if pieces.len() < 300 || pieces.len() > 1_000_000 {
            return err("suspicious piece count");
        }

        let n = pieces.len() as u32;
        let mut id_of = HashMap::with_capacity(n as usize);
        let mut mergeable = HashMap::with_capacity(n as usize);
        let mut kind = Vec::with_capacity(n as usize);
        let mut text = Vec::with_capacity(n as usize);
        let mut byte_by_id = vec![0u8; n as usize];
        let mut byte_id = [u32::MAX; 256];
        let mut user_pieces: Vec<String> = Vec::new();
        let mut unk_id = u32::MAX;

        for (i, (piece, score, typ)) in pieces.iter().enumerate() {
            let i = i as u32;
            id_of.insert(piece.clone(), i);
            match typ {
                1 | 4 | 5 => {
                    // NORMAL | USER_DEFINED | UNUSED: mergeable per the C++
                    // InitializePieces.
                    if *typ == 4 {
                        user_pieces.push(piece.clone());
                    }
                    mergeable.insert(piece.clone(), *score);
                    kind.push(PieceKind::Text);
                    text.push(piece.clone());
                }
                2 => {
                    // <unk>: derive the id from the piece (C++ does the same
                    // matcher update); the TrainerSpec unk_id is ignored.
                    if unk_id != u32::MAX {
                        return Err("more than one <unk> piece".to_string());
                    }
                    unk_id = i;
                    kind.push(PieceKind::Unk);
                    text.push(String::new());
                }
                3 => {
                    kind.push(PieceKind::Control);
                    text.push(String::new());
                }
                6 => {
                    // "<0xNN>" -> NN
                    let hex = match piece.strip_prefix("<0x").and_then(|s| s.strip_suffix('>')) {
                        Some(h) => h,
                        None => return Err(format!("bad byte piece {piece:?}")),
                    };
                    let v = u8::from_str_radix(hex, 16)
                        .map_err(|e| format!("bad byte piece {piece:?}: {e}"))?;
                    if byte_id[v as usize] != u32::MAX {
                        return Err(format!("duplicate byte piece for 0x{v:02X}"));
                    }
                    byte_id[v as usize] = i;
                    kind.push(PieceKind::Byte);
                    byte_by_id[i as usize] = v;
                    text.push(String::new());
                }
                t => return Err(format!("unknown piece type {t}")),
            }
        }

        // Fallback: all 256 byte pieces must exist (byte_fallback is
        // validated).
        if byte_id.contains(&u32::MAX) {
            return Err("byte pieces incomplete".to_string());
        }

        let user = {
            let mut u = user_pieces;
            u.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| b.cmp(a)));
            u.dedup();
            u
        };

        let unk_surface = match unk_surface_raw {
            Some(raw) => String::from_utf8_lossy(&raw).into_owned(),
            None => " \u{2047} ".to_string(),
        };

        Ok(Self {
            id_of,
            mergeable,
            kind,
            text,
            byte_by_id,
            byte_id,
            user,
            unk_surface,
        })
    }

    /// Encode `text` (the serialized tool list) to token ids, exactly as the
    /// Python package's `sp.encode(text)` does.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        // 1. Strip leading spaces; empty -> [].
        let start = text.chars().take_while(|&c| c == ' ').count();
        if text.get(start..).is_none_or(str::is_empty) {
            return Vec::new();
        }
        let rest = &text[start..];

        // 2. Escape: leading ▁ + (' ' -> ▁ with run collapse).
        let mut norm = String::with_capacity(rest.len() + 1);
        norm.push('▁');
        let mut prev_space = false;
        for c in rest.chars() {
            if prev_space && c == ' ' {
                continue;
            }
            if c == ' ' {
                norm.push('▁');
            } else {
                norm.push(c);
            }
            prev_space = c == ' ';
        }
        // 3. Trim trailing ▁.
        while norm.ends_with('▁') {
            norm.pop();
        }

        if norm.as_str() == "▁" {
            return Vec::new();
        }

        // 4. Split: frozen user pieces, else one char.
        let mut pieces: Vec<String> = Vec::new();
        let mut frozen: Vec<bool> = Vec::new();
        let mut i = 0;
        let bytes = norm.as_bytes();
        while i < bytes.len() {
            let rest = &norm[i..];
            let mut matched = None;
            for u in &self.user {
                if rest.starts_with(u.as_str()) {
                    matched = Some(u);
                    break;
                }
            }
            if let Some(u) = matched {
                pieces.push(u.clone());
                frozen.push(true);
                i += u.len();
            } else {
                let c = rest.chars().next().unwrap();
                let c = c.to_string();
                // C++ UDS: a char with no single-char vocab piece becomes
                // a byte piece ("<0xNN>", already frozen in the merge
                // heap); only vocab-char pieces are BPE-mergeable.
                if self.id_of.contains_key(c.as_str()) {
                    frozen.push(false);
                } else {
                    frozen.push(true);
                }
                i += c.len();
                pieces.push(c);
            }
        }

        let n = pieces.len() as i32;
        let mut next: Vec<i32> = (1..=n).chain(std::iter::once(-1)).collect();
        let mut prev: Vec<i32> = (-1..n).collect();

        // 5. Merge heap.
        //    Ord: higher score first; ties -> lower left first; ties ->
        //    lower right first (total order for BinaryHeap stability).
        #[derive(Clone, Copy)]
        struct Entry {
            score: f32,
            left: i32,
            right: i32,
            size: usize,
        }
        impl PartialEq for Entry {
            fn eq(&self, o: &Self) -> bool {
                self.left == o.left && self.right == o.right
            }
        }
        impl Eq for Entry {}
        impl PartialOrd for Entry {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Entry {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                self.score
                    .partial_cmp(&o.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(o.left.cmp(&self.left))
                    .then(o.right.cmp(&self.right))
            }
        }

        let mut heap = std::collections::BinaryHeap::new();
        // Push an adjacent non-frozen pair whose concatenation is a
        // mergeable vocab piece (the C++ MaybeAddNewSymbolPair). Plain
        // fn, not a closure: the merge loop mutates `pieces` in place
        // between calls, so a capturing closure's borrows would last
        // too long (E0502).
        fn push_pair(
            heap: &mut std::collections::BinaryHeap<Entry>,
            l: i32,
            r: i32,
            pieces: &[String],
            frozen: &[bool],
            mergeable: &std::collections::HashMap<String, f32>,
            n: i32,
        ) {
            if l < 0 || r < 0 || l >= n || r >= n {
                return;
            }
            let lu = l as usize;
            let ru = r as usize;
            if frozen[lu] || frozen[ru] {
                return;
            }
            let a = pieces[lu].as_str();
            let b = pieces[ru].as_str();
            if a.is_empty() || b.is_empty() {
                return;
            }
            let mut concat = String::with_capacity(a.len() + b.len());
            concat.push_str(a);
            concat.push_str(b);
            if let Some(&score) = mergeable.get(&concat) {
                heap.push(Entry {
                    score,
                    left: l,
                    right: r,
                    size: concat.len(),
                });
            }
        }
        for l in 0..n {
            push_pair(&mut heap, l, l + 1, &pieces, &frozen, &self.mergeable, n);
        }

        while let Some(top) = heap.pop() {
            // Stale-entry checks (exact C++ bpe_model.cc):
            let l = top.left;
            let r = top.right;
            let lu = l as usize;
            let ru = r as usize;
            let stale = pieces[lu].is_empty()
                || pieces[ru].is_empty()
                || pieces[lu].len() + pieces[ru].len() != top.size;
            if std::env::var("SPM_TRACE").is_ok() {
                eprintln!(
                    "POP score {:?} {} + {} stale={}",
                    top.score,
                    pieces[lu].as_str(),
                    pieces[ru].as_str(),
                    stale
                );
            }
            if stale {
                continue;
            }
            // Merge r into l (invariant l < r on array indices;
            // split_at_mut keeps the two borrows disjoint).
            let (left, right) = pieces.split_at_mut(ru);
            left[lu].push_str(&right[0]);
            let nr = next[ru];
            if nr >= 0 {
                prev[nr as usize] = l;
            }
            next[lu] = nr;
            right[0].clear();
            push_pair(&mut heap, prev[lu], l, &pieces, &frozen, &self.mergeable, n);
            let nl = next[lu];
            if nl >= 0 {
                push_pair(&mut heap, l, nl, &pieces, &frozen, &self.mergeable, n);
            }
        }

        // 6. Final ids.
        let mut ids: Vec<u32> = Vec::new();
        let mut i: i32 = 0;
        while i >= 0 && i < n {
            let t = pieces[i as usize].as_str();
            if !t.is_empty() {
                if let Some(&id) = self.id_of.get(t) {
                    ids.push(id);
                } else {
                    // A single unmatched character: byte fallback.
                    for &b in t.as_bytes() {
                        ids.push(self.byte_id[b as usize]);
                    }
                }
            }
            i = next[i as usize];
        }
        ids
    }

    /// Decode ids to text, mirroring `SentencePieceProcessor::Decode`
    /// (sentencepiece_processor.cc): control pieces emit nothing, `<unk>`
    /// emits `unk_surface`, byte runs are reassembled via
    /// [`detok_byte_run`], and the first piece that is still at the
    /// leading-whitespace position drops one leading `▁`; every other `▁`
    /// becomes a space. Stripping structural wrapper tokens around the
    /// payload is the caller's responsibility.
    pub fn decode(&self, ids: &[u32]) -> Result<String> {
        let mut out = String::new();
        let mut is_bos_ws = true;
        let mut bos_ws_seen = false;
        let mut bytes: Vec<u8> = Vec::new();
        for &id in ids {
            let Some(&kind) = self.kind.get(id as usize) else {
                // C++: Decode throws Invalid id -> the Python binding
                // surfaces this as IndexError (OUT_OF_RANGE).
                return Err(DeciderError::InvalidTokenId {
                    id,
                    vocab: self.kind.len() as u32,
                });
            };
            if matches!(kind, PieceKind::Byte) {
                // Byte pieces are buffered, not decoded in place (C++
                // ProcessBytePieces runs on maximal runs).
                bytes.push(self.byte_by_id[id as usize]);
                continue;
            }
            if !bytes.is_empty() {
                out.push_str(&detok_byte_run(&bytes));
                bytes.clear();
            }
            // C++: if (bos_ws_seen || !text->empty()) is_bos_ws = false;
            // Note this runs *before* decoding the current piece, and the
            // position stays open while the output is still empty.
            if bos_ws_seen || !out.is_empty() {
                is_bos_ws = false;
            }
            let mut has_bos_ws = false;
            let decoded: Option<String> = match kind {
                // C++ control branch: returns (empty, false).
                PieceKind::Control => None,
                // C++ unknown branch: returns (unk_surface, false) verbatim.
                PieceKind::Unk => Some(self.unk_surface.clone()),
                PieceKind::Text => {
                    let t = self.text[id as usize].as_str();
                    // The loader validates add_dummy_prefix and
                    // remove_extra_whitespaces, so the C++ gating condition
                    // (is_bos_ws && (add_dummy || remove_extra)) reduces to
                    // is_bos_ws.
                    let t = if is_bos_ws {
                        match t.strip_prefix('▁') {
                            Some(s) => {
                                has_bos_ws = true;
                                s
                            }
                            None => t,
                        }
                    } else {
                        t
                    };
                    Some(t.chars().map(|c| if c == '▁' { ' ' } else { c }).collect())
                }
                PieceKind::Byte => unreachable!(),
            };
            // C++: bos_ws_seen = has_bos_ws (set after decode, from the
            // piece's own decision).
            if has_bos_ws {
                bos_ws_seen = true;
            }
            if let Some(d) = decoded {
                out.push_str(&d);
            }
        }
        if !bytes.is_empty() {
            out.push_str(&detok_byte_run(&bytes));
        }
        Ok(out)
    }

    /// The id of the given piece text, if any.
    #[allow(dead_code)]
    pub fn id_of(&self, text: &str) -> Option<u32> {
        self.id_of.get(text).copied()
    }

    /// The text of a piece id, if the id exists.
    #[allow(dead_code)]
    pub fn piece_of(&self, id: u32) -> Option<&str> {
        self.text
            .get(id as usize)
            .filter(|_| matches!(self.kind.get(id as usize), Some(PieceKind::Text)))
            .map(|s| s.as_str())
    }
}

/// Reassemble a maximal run of byte pieces into UTF-8, mirroring the C++
/// `ProcessBytePieces`: greedily emit every structurally valid UTF-8
/// character; each byte that is not part of one becomes `U+FFFD`.
fn detok_byte_run(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let (valid_lead, lead_len) = match bytes[i] {
            0x00..=0x7f => (true, 1),
            0xc2..=0xdf => (true, 2),
            0xe0..=0xef => (true, 3),
            0xf0..=0xf4 => (true, 4),
            // Continuation bytes (0x80..=0xc1) and 0xf5..=0xff are not
            // valid lead bytes: one U+FFFD each.
            _ => (false, 1),
        };
        if valid_lead && i + lead_len <= bytes.len() {
            if let Ok(word) = std::str::from_utf8(&bytes[i..i + lead_len]) {
                s.push_str(word);
                i += lead_len;
                continue;
            }
        }
        s.push('\u{fffd}');
        i += 1;
    }
    s
}

/// Minimal proto2 wire-format reader over a byte slice (ModelProto schema
/// only; tags we do not know are skipped).
struct Proto<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Proto<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_varint(&mut self) -> Option<u64> {
        let mut v: u64 = 0;
        let mut shift = 0;
        loop {
            if self.pos >= self.data.len() {
                return None;
            }
            let b = self.data[self.pos];
            self.pos += 1;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }

    /// Skips one value of the given wire type and returns the byte range of
    /// the value itself. For wire type 2 the range is the payload (excluding
    /// the leading length varint), so an inner message parses from its first
    /// tag and a string field yields its bytes.
    fn skip_value(&mut self, wire: u32) -> Option<std::ops::Range<usize>> {
        match wire {
            0 => {
                let start = self.pos;
                self.read_varint()?;
                Some(start..self.pos)
            }
            1 => {
                let start = self.pos;
                if start + 8 > self.data.len() {
                    return None;
                }
                self.pos = start + 8;
                Some(start..self.pos)
            }
            2 => {
                let len = self.read_varint()? as usize;
                let start = self.pos;
                if start + len > self.data.len() {
                    return None;
                }
                self.pos = start + len;
                Some(start..self.pos)
            }
            5 => {
                let start = self.pos;
                if start + 4 > self.data.len() {
                    return None;
                }
                self.pos = start + 4;
                Some(start..self.pos)
            }
            _ => None,
        }
    }

    /// Visit each field whose number is in `wanted`, calling
    /// `f(num, wire, value_bytes)` for it. `value_bytes` is the field's value
    /// span: for wire type 2 the payload (inner message / string) without the
    /// length prefix, for wire type 0 the varint bytes, and for wire types
    /// 1 / 5 the raw 8 / 4 bytes.
    fn each_field<F>(&mut self, wanted: &[u32], mut f: F) -> std::result::Result<(), String>
    where
        F: FnMut(u32, u32, &[u8]) -> std::result::Result<(), String>,
    {
        while self.pos < self.data.len() {
            let tag = self
                .read_varint()
                .ok_or_else(|| "truncated tag".to_string())?;
            let num = ((tag >> 3) & 0x1FFF) as u32;
            let wire = (tag & 0x7) as u32;
            let span = self
                .skip_value(wire)
                .ok_or_else(|| format!("truncated field {num}"))?;
            if wanted.contains(&num) {
                f(num, wire, &self.data[span])?;
            }
        }
        Ok(())
    }
}

/// Interpret a field value's raw bytes per wire type.
fn as_varint(b: &[u8]) -> u64 {
    let mut v: u64 = 0;
    let mut shift = 0;
    for &byte in b {
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    v
}

fn as_string(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn as_f32(b: &[u8]) -> f32 {
    f32::from_bits(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Load the real model file: `NEUDECIDE_MODEL_DIR`, else the first HF
    /// snapshot that ships a `tokenizer.model`. Returns `None` to skip
    /// when absent (CI without the model).
    fn load_model() -> Option<(&'static str, std::sync::Arc<SpmTokenizer>)> {
        let dir = std::env::var("NEUDECIDE_MODEL_DIR")
            .ok()
            .map(std::path::PathBuf::from)
            .filter(|p| p.join("tokenizer.model").is_file())
            .or_else(|| {
                let home = std::env::var("HOME").ok()?;
                let base = std::path::PathBuf::from(home)
                    .join(".cache/huggingface/hub/models--neuphonic--neudecide/snapshots");
                std::fs::read_dir(&base)
                    .ok()?
                    .filter_map(|p| p.ok())
                    .map(|e| e.path())
                    .find(|p| p.join("tokenizer.model").is_file())
            })?;
        let tok = SpmTokenizer::load(&dir.join("tokenizer.model")).ok()?;
        Some(("model", std::sync::Arc::new(tok)))
    }

    /// Oracle fixture: `encode`: text -> ids; `decode`: "id,id" -> text;
    /// `decode_err`: lists of ids that must fail with `InvalidTokenId`.
    /// Generated from the Python `sentencepiece` wheel the `neudecide`
    /// package runs on (see `tasks/neudecide-spm-oracle`).
    fn oracle() -> serde_json::Value {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/spm_oracle.json"
        ))
        .expect("fixture present in repo");
        serde_json::from_str(&raw).unwrap()
    }

    #[test]
    fn fixture_encode_parity() {
        let Some((_, spm)) = load_model() else {
            eprintln!("SKIP: model dir absent");
            return;
        };
        let v = oracle();
        for (text, ids) in v["encode"].as_object().unwrap() {
            let expected: Vec<u32> = ids
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_u64().unwrap() as u32)
                .collect();
            let got = spm.encode(text);
            assert_eq!(got, expected, "encode parity for {text:?}");
        }
    }

    #[test]
    fn fixture_decode_parity() {
        let Some((_, spm)) = load_model() else {
            eprintln!("SKIP: model dir absent");
            return;
        };
        let v = oracle();
        for (ids, text) in v["decode"].as_object().unwrap() {
            let ids: Vec<u32> = ids.split(',').map(|s| s.trim().parse().unwrap()).collect();
            let got = spm.decode(&ids).expect("in-range ids must decode");
            assert_eq!(got, text.as_str().unwrap(), "decode parity for {ids:?}");
        }
    }

    #[test]
    fn fixture_decode_errors() {
        let Some((_, spm)) = load_model() else {
            eprintln!("SKIP: model dir absent");
            return;
        };
        let v = oracle();
        for ids in v["decode_err"].as_array().unwrap() {
            let ids: Vec<u32> = ids
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_u64().unwrap() as u32)
                .collect();
            let got = spm.decode(&ids);
            assert!(
                matches!(got, Err(DeciderError::InvalidTokenId { .. })),
                "expected InvalidTokenId for {ids:?}, got {got:?}",
            );
        }
    }

    /// Known-value invariants (run against the real model, skipped when
    /// absent): pins the algorithm's observable behavior without the
    /// oracle fixture.
    #[test]
    fn quick_invariants() {
        let Some((_, spm)) = load_model() else {
            eprintln!("SKIP: model dir absent");
            return;
        };
        assert_eq!(spm.encode(""), Vec::<u32>::new());
        assert!(spm.encode("   ").is_empty());
        // '<tools>' is a single user piece: '▁<tools>' -> [id("▁"), id("<tools>")].
        let tid = spm.id_of("<tools>").unwrap();
        assert_eq!(spm.encode("<tools>"), vec![spm.id_of("▁").unwrap(), tid],);
        // Same for the other user-defined piece (frozen, single token).
        let toid = spm.id_of("<tool_call>").unwrap();
        assert_eq!(
            spm.encode("<tool_call>"),
            vec![spm.id_of("▁").unwrap(), toid],
        );
        // User pieces are frozen: '<tools>a' must not merge with its
        // neighbors into a longer vocab piece.
        assert_eq!(
            spm.encode("a<tools>"),
            vec![spm.id_of("▁a").expect("vocab has ▁a"), tid,],
        );
        // decode: control pieces emit nothing.
        assert_eq!(spm.decode(&[0, 1, 2]).unwrap(), "");
        // decode: <unk> -> unk_surface verbatim.
        let unk = spm.id_of("<unk>").unwrap();
        assert_eq!(spm.decode(&[unk]).unwrap(), " \u{2047} ");
        // decode: byte runs reassemble (C2 B2 = ²).
        let b_c2 = spm.id_of("<0xC2>").unwrap();
        let b_b2 = spm.id_of("<0xB2>").unwrap();
        assert_eq!(spm.decode(&[b_c2, b_b2]).unwrap(), "²");
        // decode: byte run mid-text, after a real piece.
        let spm_a = spm.id_of("▁a").expect("vocab has ▁a");
        assert_eq!(spm.decode(&[spm_a, b_c2, b_b2]).unwrap(), "a²");
        // decode: out-of-range id is an error (Python: IndexError).
        assert!(matches!(
            spm.decode(&[8192]),
            Err(DeciderError::InvalidTokenId {
                id: 8192,
                vocab: 8192
            })
        ));
    }

    /// The v1 tool JSON as built for the wire — the exact conditioning text
    /// the ort path encodes (`tools_to_json` over `v1_tools`). Equivalence
    /// with Python at the id level is proven by `parity_with_python` in
    /// `tests/a4_no_mic.rs` (50-clip gate); this pins the invariants the
    /// broken BPE fallback violated (zero merge rules → ~1830 single-byte
    /// ids, lossy round trip): sane piece count, no UNK, lossless round trip.
    #[test]
    fn v1_tool_json_encode_invariants() {
        let Some((_, spm)) = load_model() else {
            eprintln!("SKIP: model dir absent");
            return;
        };
        let mk = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let apps = mk(&["terminal", "safari", "notes", "mail", "finder", "code"]);
        let snippets = mk(&["thanks", "regards", "br", "signature"]);
        let styles = mk(&["body", "heading", "code"]);
        let json =
            crate::tool_json::tools_to_json(&crate::tool_json::v1_tools(&apps, &snippets, &styles));
        let ids = spm.encode(&json);
        let unk = spm.id_of("<unk>").expect("vocab has <unk>");
        assert!(!ids.is_empty());
        assert_eq!(
            ids.first().copied(),
            spm.id_of("▁[{\""),
            "SPM encode re-anchors on U+2581 and merges the leading JSON opener like Python (first id 356)"
        );
        assert!(!ids.contains(&unk), "no <unk> in the tool JSON encode");
        assert!(
            100 < ids.len() && ids.len() < 2000,
            "piece count {} is neither single-byte collapse (~1830+) nor degenerate",
            ids.len()
        );
        assert_eq!(spm.decode(&ids).expect("round trip"), json);
    }

    #[test]
    fn probe() {
        let Some((_, spm)) = load_model() else {
            eprintln!("SKIP: model dir absent");
            return;
        };
        for id in [
            8041u32, 5, 16, 8071, 8072, 66, 8106, 369, 425, 5832, 8046, 68, 3,
        ] {
            let t = spm.decode(&[id]).unwrap_or_default();
            let kind = spm.kind[id as usize];
            let raw = spm.text[id as usize].as_str();
            println!("id {id} -> kind {kind:?} text {raw:?} decode {t:?}");
        }
        for s in [
            "t", "o", "l", "s", "to", "ol", "too", "ool", "ool>", "tool", "ols", "ls", "s>", "▁to",
            "▁ol", "▁too", "▁ool", "▁tool", "<", "/", "▁<", "▁/", "</t", "▁</t", "oo", "/t", "/to",
            "</",
        ] {
            let key = spm
                .id_of(s)
                .map(|i| i.to_string())
                .unwrap_or_else(|| "ABSENT".into());
            let score = spm
                .mergeable
                .get(s)
                .map(|sc| sc.to_string())
                .unwrap_or_else(|| "no".into());
            println!("key {s:?} -> id {key} mergeable {score}");
        }
    }
}
