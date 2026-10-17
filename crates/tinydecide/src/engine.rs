//! The encoder and the typed heads. A line-by-line port of `TinyDecide` in tinydecide.js.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::tokenizer::{is_js_space, WordPiece};
use crate::weights::{Meta, Tensor, Weights};

const K_STATE: u8 = 0;
const K_QTEXT: u8 = 1;
const K_ANS: u8 = 2;
const K_OPT: u8 = 3;
const K_LV: u8 = 4;
const K_BUCKETS: [i32; 4] = [1, 2, 4, 8];

pub(crate) fn bucket(c: i32) -> usize {
    K_BUCKETS.iter().filter(|&&e| c > e).count()
}

/// The four answer types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QType {
    /// Pick one of 2 to 32 options.
    Choice,
    /// Is a statement about the message true?
    Noul,
    /// Place the message on ordered levels, lowest first.
    Score,
    /// Extract a piece of the message.
    Span,
}

impl QType {
    pub fn as_str(self) -> &'static str {
        match self {
            QType::Choice => "choice",
            QType::Noul => "noul",
            QType::Score => "score",
            QType::Span => "span",
        }
    }
}

/// One question, written in plain language at call time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    #[serde(rename = "type")]
    pub kind: QType,
    pub text: String,
    /// Options (choice) or levels, lowest first (score). Must be empty for noul and span.
    #[serde(default)]
    pub options: Vec<String>,
}

impl Question {
    pub fn choice<S: AsRef<str>>(text: &str, options: &[S]) -> Self {
        Question {
            kind: QType::Choice,
            text: text.into(),
            options: options.iter().map(|o| o.as_ref().to_string()).collect(),
        }
    }
    pub fn noul(text: &str) -> Self {
        Question {
            kind: QType::Noul,
            text: text.into(),
            options: vec![],
        }
    }
    pub fn score<S: AsRef<str>>(text: &str, levels: &[S]) -> Self {
        Question {
            kind: QType::Score,
            text: text.into(),
            options: levels.iter().map(|o| o.as_ref().to_string()).collect(),
        }
    }
    pub fn span(text: &str) -> Self {
        Question {
            kind: QType::Span,
            text: text.into(),
            options: vec![],
        }
    }
}

/// Corrections for one question (see [`crate::corrections::make_protos`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Protos {
    /// One prototype per option (2 for noul: false, true), `n_options * qvec_len` values.
    pub vec: Vec<f32>,
    /// Examples behind each prototype; options with 0 add nothing.
    pub cnt: Vec<i32>,
    /// Mean `qvec` the cosines are centred on. `None` centres them on the count-weighted mean of
    /// the prototypes, as tinydecide.js does.
    pub center: Option<Vec<f32>>,
    /// Trust weight. `None` means 1.
    pub lam: Option<f64>,
}

/// One answer. Which fields are set depends on `kind`, as in the JavaScript result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    pub kind: QType,
    /// choice / score: one probability per option.
    pub probs: Vec<f64>,
    /// choice / score: index of the most likely option.
    pub pick: Option<usize>,
    /// choice / score: 1 - normalised entropy.
    pub confidence: Option<f64>,
    /// score: expected level from 0 (first) to 1 (last).
    pub score: Option<f64>,
    /// noul: probability the statement is true.
    pub p: Option<f64>,
    /// choice / score / noul: the vector to store with a correction.
    pub qvec: Vec<f32>,
    /// True when `qvec` is the prototype projection (every released model).
    pub proj: bool,
    /// choice / score / noul: zero-shot logits over temperature (noul: `[0, z]`), stored with a correction.
    pub z0: Vec<f64>,
    /// span: probability the message contains what was asked for.
    pub p_present: Option<f64>,
    /// span: probability of the chosen start and end tokens.
    pub p_span: Option<f64>,
    /// span: first and last state token of the span (0-based, without the state marker).
    pub tok: Option<(usize, usize)>,
    /// span: the extracted text, trimmed.
    pub text: Option<String>,
    /// span: `(start, end)` **UTF-8 byte offsets** into the state string (JavaScript reports UTF-16 units).
    pub char: Option<(usize, usize)>,
}

impl Answer {
    fn empty(kind: QType) -> Self {
        Answer {
            kind,
            probs: vec![],
            pick: None,
            confidence: None,
            score: None,
            p: None,
            qvec: vec![],
            proj: false,
            z0: vec![],
            p_present: None,
            p_span: None,
            tok: None,
            text: None,
            char: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Tokens {
    /// State tokens including the state marker.
    pub state: usize,
    pub questions: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Response {
    pub answers: Vec<Answer>,
    pub ids: Vec<u32>,
    pub tokens: Tokens,
    /// The message was longer than the model reads (`format.ts_max - 1` tokens) and was cut.
    pub truncated: bool,
    /// Time spent in the call, in milliseconds (0 on `wasm32-unknown-unknown`, which has no clock).
    pub ms: f64,
}

// ------------------------------------------------------------------ timing

/// Wall-clock time for `Response::ms`. Bare `wasm32-unknown-unknown` has no clock in `std`
/// (`Instant::now` traps there), so that target reports 0.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
struct Clock(std::time::Instant);
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl Clock {
    fn now() -> Self {
        Clock(std::time::Instant::now())
    }
    fn ms(&self) -> f64 {
        self.0.elapsed().as_secs_f64() * 1000.0
    }
}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
struct Clock;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl Clock {
    fn now() -> Self {
        Clock
    }
    fn ms(&self) -> f64 {
        0.0
    }
}

// ------------------------------------------------------------------ math

// 16 independent accumulators so the loop vectorises
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let mut acc = [0f32; 16];
    let ca = a.chunks_exact(16);
    let cb = b.chunks_exact(16);
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        for i in 0..16 {
            acc[i] += x[i] * y[i];
        }
    }
    let mut s = 0f32;
    for v in acc {
        s += v;
    }
    for (x, y) in ra.iter().zip(rb) {
        s += x * y;
    }
    s
}

/// `dot(w, x_k)` for the four rows of `x` (each `w.len()` long), summed in the same order as
/// `dot`, so the results are bit-identical.
#[inline]
fn dot4(w: &[f32], x: &[f32]) -> [f32; 4] {
    let n = w.len();
    let (x0, r) = x.split_at(n);
    let (x1, r) = r.split_at(n);
    let (x2, x3) = r.split_at(n);
    let mut acc = [[0f32; 16]; 4];
    let chunks = w
        .chunks_exact(16)
        .zip(x0.chunks_exact(16))
        .zip(x1.chunks_exact(16))
        .zip(x2.chunks_exact(16))
        .zip(x3.chunks_exact(16));
    for ((((wc, c0), c1), c2), c3) in chunks {
        let wc: &[f32; 16] = wc.try_into().unwrap();
        let c: [&[f32; 16]; 4] = [
            c0.try_into().unwrap(),
            c1.try_into().unwrap(),
            c2.try_into().unwrap(),
            c3.try_into().unwrap(),
        ];
        for ((a, wv), x) in acc[0].iter_mut().zip(wc.iter()).zip(c[0].iter()) {
            *a += wv * x;
        }
        for ((a, wv), x) in acc[1].iter_mut().zip(wc.iter()).zip(c[1].iter()) {
            *a += wv * x;
        }
        for ((a, wv), x) in acc[2].iter_mut().zip(wc.iter()).zip(c[2].iter()) {
            *a += wv * x;
        }
        for ((a, wv), x) in acc[3].iter_mut().zip(wc.iter()).zip(c[3].iter()) {
            *a += wv * x;
        }
    }
    let tail = n - n % 16;
    let xs = [x0, x1, x2, x3];
    let mut out = [0f32; 4];
    for k in 0..4 {
        let mut s = 0f32;
        for v in acc[k] {
            s += v;
        }
        for (av, bv) in w[tail..].iter().zip(&xs[k][tail..]) {
            s += av * bv;
        }
        out[k] = s;
    }
    out
}

fn dot64(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(&x, &y)| x as f64 * y as f64).sum()
}

fn dotd(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm64(a: &[f32]) -> Vec<f64> {
    let n = dot64(a, a).sqrt();
    let n = if n == 0.0 || n.is_nan() { 1e-12 } else { n };
    a.iter().map(|&z| z as f64 / n).collect()
}

struct Lin {
    w: Vec<f32>,
    b: Option<Vec<f32>>,
    rows: usize,
    cols: usize,
}

impl Lin {
    /// y[t] = W x[t] + b for T rows of x.
    fn apply(&self, x: &[f32], t: usize) -> Vec<f32> {
        let (din, dout) = (self.cols, self.rows);
        let mut y = vec![0f32; t * dout];
        // Tiling needs vector registers: wasm32 without simd128 runs faster one token at a time.
        const TILE: bool = cfg!(any(not(target_arch = "wasm32"), target_feature = "simd128"));
        let t4 = if TILE { t - t % 4 } else { 0 };
        for j in 0..dout {
            let row = &self.w[j * din..(j + 1) * din];
            let b = self.b.as_ref().map_or(0.0, |b| b[j]);
            // four tokens per pass share each load of the weight row
            for tt in (0..t4).step_by(4) {
                let r = dot4(row, &x[tt * din..(tt + 4) * din]);
                for (k, rk) in r.into_iter().enumerate() {
                    y[(tt + k) * dout + j] = b + rk;
                }
            }
            for tt in t4..t {
                y[tt * dout + j] = b + dot(row, &x[tt * din..(tt + 1) * din]);
            }
        }
        y
    }
    fn mv(&self, v: &[f32]) -> Vec<f32> {
        self.apply(v, 1)
    }
}

fn layernorm(x: &[f32], t: usize, d: usize, w: &[f32], b: Option<&[f32]>, eps: f64) -> Vec<f32> {
    let mut y = vec![0f32; t * d];
    for tt in 0..t {
        let r = &x[tt * d..(tt + 1) * d];
        let m = r.iter().map(|&z| z as f64).sum::<f64>() / d as f64;
        let v = r
            .iter()
            .map(|&z| (z as f64 - m) * (z as f64 - m))
            .sum::<f64>()
            / d as f64;
        let inv = 1.0 / (v + eps).sqrt();
        for i in 0..d {
            let bi = b.map_or(0.0, |b| b[i] as f64);
            y[tt * d + i] = ((r[i] as f64 - m) * inv * w[i] as f64 + bi) as f32;
        }
    }
    y
}

/// The erf used by tinydecide.js: a Taylor series below 0.5, the Numerical Recipes erfc fit above.
fn erf(x: f64) -> f64 {
    let s = if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        return x;
    };
    let a = x.abs();
    if a < 0.5 {
        let (mut sum, mut term, a2) = (a, a, a * a);
        for n in 1..12 {
            term *= -a2 / n as f64;
            sum += term / (2 * n + 1) as f64;
        }
        return s * 2.0 / std::f64::consts::PI.sqrt() * sum;
    }
    let t = 1.0 / (1.0 + 0.5 * a);
    let y = t
        * (-a * a - 1.26551223
            + t * (1.00002368
                + t * (0.37409196
                    + t * (0.09678418
                        + t * (-0.18628806
                            + t * (0.27886807
                                + t * (-1.13520398
                                    + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277)))))))))
            .exp();
    s * (1.0 - y)
}

fn gelu(x: f32) -> f32 {
    let x = x as f64;
    (0.5 * x * (1.0 + erf(x / std::f64::consts::SQRT_2))) as f32
}

/// log-softmax of a / tt.
fn lsm(a: &[f64], tt: f64) -> Vec<f64> {
    let m = a.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let l = a.iter().map(|z| ((z - m) / tt).exp()).sum::<f64>().ln();
    a.iter().map(|z| (z - m) / tt - l).collect()
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

// ------------------------------------------------------------------ model

struct Block {
    q: Lin,
    k: Lin,
    v: Lin,
    o: Lin,
    ln1: (Vec<f32>, Vec<f32>),
    fc: Lin,
    fc2: Lin,
    ln2: (Vec<f32>, Vec<f32>),
}

enum WordTable {
    Full(Vec<f32>),
    LowRank { a: Vec<f32>, b: Vec<f32>, r: usize },
}

struct Heads {
    norm: (Vec<f32>, Vec<f32>),
    scale: Vec<f32>,
    a: [Lin; 2],
    o: [Lin; 2],
    p: Option<Lin>,
    noul_w: Vec<f32>,
    noul_b: f32,
    noul_q: Lin,
    sq: Lin,
    sk: Lin,
    snull_w: Vec<f32>,
    snull_b: f32,
    eq: Lin,
    es: Lin,
    ek: Lin,
}

struct Specials {
    state: u32,
    types: [u32; 4],
    sep: u32,
    o: u32,
    lv: u32,
    ans: u32,
}

struct QEnc {
    kind: QType,
    ans: usize,
    opt: Vec<usize>,
}

struct Enc {
    ids: Vec<u32>,
    pos: Vec<usize>,
    blk: Vec<usize>,
    kind: Vec<u8>,
    st_idx: Vec<usize>,
    st_off: Vec<(usize, usize)>,
    qs: Vec<QEnc>,
    truncated: bool,
}

/// A loaded TinyDecide model.
pub struct TinyDecide {
    meta: Meta,
    tok: WordPiece,
    sp: Specials,
    word: WordTable,
    pos: Vec<f32>,
    type0: Vec<f32>,
    eln: (Vec<f32>, Vec<f32>),
    proj: Lin,
    blocks: Vec<Block>,
    heads: Heads,
}

fn take_vec(w: &mut Weights, n: &str, len: usize) -> Result<Vec<f32>> {
    Ok(w.take(n, &[Some(len)])?.data)
}

fn take_lin(
    w: &mut Weights,
    n: &str,
    rows: Option<usize>,
    cols: usize,
    bias: Option<&str>,
) -> Result<Lin> {
    let t: Tensor = w.take(n, &[rows, Some(cols)])?;
    let rows = t.shape[0];
    let b = match bias {
        Some(bn) => Some(take_vec(w, bn, rows)?),
        None => None,
    };
    Ok(Lin {
        w: t.data,
        b,
        rows,
        cols,
    })
}

impl TinyDecide {
    /// Loads `meta.json` and `model.bin` from a directory.
    pub fn load<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let dir = dir.as_ref();
        let meta = std::fs::read_to_string(dir.join("meta.json"))?;
        let bin = std::fs::read(dir.join("model.bin"))?;
        Self::from_bytes(&meta, &bin)
    }

    /// Builds a model from the text of `meta.json` and the bytes of `model.bin`.
    pub fn from_bytes(meta_json: &str, bin: &[u8]) -> Result<Self> {
        let mut meta: Meta = serde_json::from_str(meta_json)?;
        let c = meta.cfg.clone();
        if c.arch != "electra" {
            return Err(Error::Model(format!("unsupported architecture {}", c.arch)));
        }
        if meta.tokenizer.kind != "wordpiece" {
            return Err(Error::Model(
                "this engine build ships the WordPiece tokenizer only".into(),
            ));
        }
        if c.heads == 0 || c.d % c.heads != 0 {
            return Err(Error::Model("cfg.d must be a multiple of cfg.heads".into()));
        }
        if meta.temp.len() < 4 || meta.beta.len() < 5 {
            return Err(Error::Model(
                "meta.temp needs 4 values and meta.beta 5".into(),
            ));
        }
        let tk = &meta.tokenizer;
        let tok = WordPiece::new(&tk.vocab, &tk.unk, &tk.prefix, tk.max_chars)?;
        let vocab_len = tk.vocab.len();
        meta.tokenizer.vocab = Vec::new();

        let mut w = Weights::new(&meta, bin)?;
        let (d, e, dh) = (c.d, c.emb_dim, c.dh_head);
        let word = if w.has("m.word.a.weight") {
            let a = w.take("m.word.a.weight", &[None, None])?;
            let r = a.shape[1];
            let b = w.take("m.word.b.weight", &[Some(e), Some(r)])?;
            (
                WordTable::LowRank {
                    a: a.data,
                    b: b.data,
                    r,
                },
                a.shape[0],
            )
        } else {
            let t = w.take("m.word.weight", &[None, Some(e)])?;
            let rows = t.shape[0];
            (WordTable::Full(t.data), rows)
        };
        let (word, vocab_rows) = word;
        let posemb = w.take("m.posemb.weight", &[None, Some(e)])?;
        let pos_rows = posemb.shape[0];
        let f = &meta.format;
        if f.ts_max == 0 || f.ts_max > pos_rows || f.p_q + f.q_max > pos_rows {
            return Err(Error::Model(
                "format.ts_max / p_q / q_max do not fit the position table".into(),
            ));
        }
        let spc = |n: &str| -> Result<u32> {
            let id = *meta
                .specials
                .get(n)
                .ok_or_else(|| Error::Model(format!("missing special token {n}")))?;
            if id as usize >= vocab_rows {
                return Err(Error::Model(format!(
                    "special token {n} is outside the embedding table"
                )));
            }
            Ok(id)
        };
        let sp = Specials {
            state: spc("<|state|>")?,
            types: [
                spc("<|choice|>")?,
                spc("<|noul|>")?,
                spc("<|score|>")?,
                spc("<|span|>")?,
            ],
            sep: spc("<|sep|>")?,
            o: spc("<|o|>")?,
            lv: spc("<|lv|>")?,
            ans: spc("<|ans|>")?,
        };
        if tok.unk() as usize >= vocab_rows {
            return Err(Error::Model(
                "the unknown token is outside the embedding table".into(),
            ));
        }
        // every vocabulary id must have an embedding row
        if vocab_len > vocab_rows {
            return Err(Error::Model(
                "the vocabulary is larger than the embedding table".into(),
            ));
        }

        let type0 = take_vec(&mut w, "m.type0", e)?;
        let eln = (
            take_vec(&mut w, "m.eln.weight", e)?,
            take_vec(&mut w, "m.eln.bias", e)?,
        );
        let proj = take_lin(&mut w, "m.proj.weight", Some(d), e, Some("m.proj.bias"))?;
        let mut blocks = Vec::with_capacity(c.layers);
        for l in 0..c.layers {
            let p = format!("m.blocks.{l}.");
            let n = |s: &str| format!("{p}{s}");
            let q = take_lin(&mut w, &n("q.weight"), Some(d), d, Some(&n("q.bias")))?;
            let k = take_lin(&mut w, &n("k.weight"), Some(d), d, Some(&n("k.bias")))?;
            let v = take_lin(&mut w, &n("v.weight"), Some(d), d, Some(&n("v.bias")))?;
            let o = take_lin(&mut w, &n("o.weight"), Some(d), d, Some(&n("o.bias")))?;
            let ln1 = (
                take_vec(&mut w, &n("ln1.weight"), d)?,
                take_vec(&mut w, &n("ln1.bias"), d)?,
            );
            let fc = take_lin(&mut w, &n("fc.weight"), None, d, Some(&n("fc.bias")))?;
            let fc2 = take_lin(
                &mut w,
                &n("fc2.weight"),
                Some(d),
                fc.rows,
                Some(&n("fc2.bias")),
            )?;
            let ln2 = (
                take_vec(&mut w, &n("ln2.weight"), d)?,
                take_vec(&mut w, &n("ln2.bias"), d)?,
            );
            blocks.push(Block {
                q,
                k,
                v,
                o,
                ln1,
                fc,
                fc2,
                ln2,
            });
        }
        let hl = |w: &mut Weights, n: &str| take_lin(w, n, Some(dh), d, None);
        let p = if w.has("h.P.weight") {
            Some(hl(&mut w, "h.P.weight")?)
        } else {
            None
        };
        let heads = Heads {
            norm: (
                take_vec(&mut w, "h.norm.weight", d)?,
                take_vec(&mut w, "h.norm.bias", d)?,
            ),
            scale: take_vec(&mut w, "h.scale", 2)?,
            a: [hl(&mut w, "h.A.0.weight")?, hl(&mut w, "h.A.1.weight")?],
            o: [hl(&mut w, "h.O.0.weight")?, hl(&mut w, "h.O.1.weight")?],
            p,
            noul_w: w.take("h.noul.weight", &[Some(1), Some(d)])?.data,
            noul_b: take_vec(&mut w, "h.noul.bias", 1)?[0],
            noul_q: hl(&mut w, "h.noul_q.weight")?,
            sq: hl(&mut w, "h.sq.weight")?,
            sk: hl(&mut w, "h.sk.weight")?,
            snull_w: w.take("h.snull.weight", &[Some(1), Some(d)])?.data,
            snull_b: take_vec(&mut w, "h.snull.bias", 1)?[0],
            eq: hl(&mut w, "h.eq.weight")?,
            es: hl(&mut w, "h.es.weight")?,
            ek: hl(&mut w, "h.ek.weight")?,
        };
        Ok(TinyDecide {
            meta,
            tok,
            sp,
            word,
            pos: posemb.data,
            type0,
            eln,
            proj,
            blocks,
            heads,
        })
    }

    /// The model's metadata (`cfg`, `format`, `temp`, `beta`, ...); the vocabulary is dropped after loading.
    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    /// `meta.beta`, the prototype weights by example count; pass it to [`crate::corrections::make_protos`].
    pub fn beta(&self) -> &[f64] {
        &self.meta.beta
    }

    pub fn tokenizer(&self) -> &WordPiece {
        &self.tok
    }

    /// Answers every question about `state` in one encoder pass.
    pub fn answer(&self, state: &str, questions: &[Question]) -> Result<Response> {
        self.answer_with(state, questions, &[])
    }

    /// Like [`answer`](Self::answer), with corrections: `protos[i]` belongs to `questions[i]`
    /// (missing entries mean none).
    pub fn answer_with(
        &self,
        state: &str,
        questions: &[Question],
        protos: &[Option<Protos>],
    ) -> Result<Response> {
        let t0 = Clock::now();
        let enc = self.encode_request(state, questions)?;
        let dh = self.meta.cfg.dh_head;
        for (qi, q) in questions.iter().enumerate() {
            if let Some(Some(p)) = protos.get(qi) {
                let k = if q.kind == QType::Noul {
                    2
                } else {
                    q.options.len()
                };
                for i in 0..k {
                    if p.cnt.get(i).copied().unwrap_or(0) > 0 && p.vec.len() < (i + 1) * dh {
                        return Err(Error::Request(format!(
                            "Corrections for question {} have too few vector values.",
                            qi + 1
                        )));
                    }
                }
                if p.center.as_ref().is_some_and(|c| c.len() < dh) {
                    return Err(Error::Request(format!(
                        "Corrections for question {} have a short center.",
                        qi + 1
                    )));
                }
            }
        }
        let x = self.hidden(&enc);
        let answers = self.heads(&enc, &x, state, protos);
        let t = enc.ids.len();
        let s = enc.st_idx.len();
        Ok(Response {
            answers,
            tokens: Tokens {
                state: s + 1,
                questions: t - s - 1,
                total: t,
            },
            truncated: enc.truncated,
            ms: t0.ms(),
            ids: enc.ids,
        })
    }

    fn encode_request(&self, state: &str, questions: &[Question]) -> Result<Enc> {
        let f = &self.meta.format;
        let st = self.tok.encode_until(state, f.ts_max - 1);
        let n = st.ids.len().min(f.ts_max - 1);
        let mut ids = Vec::with_capacity(n + 1 + questions.len() * 16);
        ids.push(self.sp.state);
        ids.extend_from_slice(&st.ids[..n]);
        let mut pos: Vec<usize> = (0..ids.len()).collect();
        let mut blk = vec![0usize; ids.len()];
        let mut kind = vec![K_STATE; ids.len()];
        let st_idx: Vec<usize> = (1..ids.len()).collect();
        let st_off = st.offsets[..n].to_vec();
        let mut qs = Vec::with_capacity(questions.len());
        for (qi, q) in questions.iter().enumerate() {
            let k = qi + 1;
            let k_max = f.k_max.unwrap_or(usize::MAX);
            let n_opt = q.options.len();
            if matches!(q.kind, QType::Choice | QType::Score) && (n_opt < 2 || n_opt > k_max) {
                let max = f.k_max.map_or("any".to_string(), |m| m.to_string());
                return Err(Error::Request(format!(
                    "Question {k} needs 2 to {max} options, not {n_opt}."
                )));
            }
            if matches!(q.kind, QType::Noul | QType::Span) && n_opt > 0 {
                return Err(Error::Request(format!(
                    "Question {k} is a {} question and takes no options.",
                    q.kind.as_str()
                )));
            }
            let ti = match q.kind {
                QType::Choice => 0,
                QType::Noul => 1,
                QType::Score => 2,
                QType::Span => 3,
            };
            let mut b_ids = vec![self.sp.types[ti]];
            let mut b_kind = vec![K_QTEXT];
            let t = self.tok.encode(&q.text).ids;
            b_kind.extend(std::iter::repeat_n(K_QTEXT, t.len()));
            b_ids.extend(t);
            let mut opt_local = Vec::new();
            if !q.options.is_empty() {
                b_ids.push(self.sp.sep);
                b_kind.push(K_QTEXT);
                let (mk, mkind) = if q.kind == QType::Choice {
                    (self.sp.o, K_OPT)
                } else {
                    (self.sp.lv, K_LV)
                };
                for o in &q.options {
                    let oi = self.tok.encode(o).ids;
                    b_kind.extend(std::iter::repeat_n(K_QTEXT, oi.len()));
                    b_ids.extend(oi);
                    opt_local.push(b_ids.len());
                    b_ids.push(mk);
                    b_kind.push(mkind);
                }
            }
            let ans_local = b_ids.len();
            b_ids.push(self.sp.ans);
            b_kind.push(K_ANS);
            if b_ids.len() > f.q_max {
                return Err(Error::Request(format!(
                    "Question {k} is too long ({} tokens, max {}).",
                    b_ids.len(),
                    f.q_max
                )));
            }
            let base = ids.len();
            for (j, (&id, &kd)) in b_ids.iter().zip(&b_kind).enumerate() {
                ids.push(id);
                pos.push(f.p_q + j);
                blk.push(k);
                kind.push(kd);
            }
            qs.push(QEnc {
                kind: q.kind,
                ans: base + ans_local,
                opt: opt_local.iter().map(|j| base + j).collect(),
            });
        }
        Ok(Enc {
            ids,
            pos,
            blk,
            kind,
            st_idx,
            st_off,
            qs,
            truncated: st.ids.len() > n,
        })
    }

    fn continues(&self, kind: &[u8]) -> Vec<bool> {
        match self.meta.cfg.fusion.as_deref() {
            Some("all") => vec![true; kind.len()],
            Some("markers") => kind
                .iter()
                .map(|&k| matches!(k, K_STATE | K_ANS | K_OPT | K_LV))
                .collect(),
            _ => kind.iter().map(|&k| k == K_STATE || k == K_ANS).collect(),
        }
    }

    fn allowed_sets(enc: &Enc, cont: &[bool], fusion_layer: bool) -> Vec<Vec<usize>> {
        let t = enc.ids.len();
        let nb = enc.blk.iter().max().map_or(0, |m| m + 1);
        let mut by_blk: Vec<Vec<usize>> = vec![Vec::new(); nb];
        for j in 0..t {
            if fusion_layer && !cont[j] {
                continue;
            }
            by_blk[enc.blk[j]].push(j);
        }
        let mut sets = Vec::with_capacity(t);
        for i in 0..t {
            if fusion_layer && !cont[i] {
                sets.push(vec![i]);
                continue;
            }
            let own = if by_blk[enc.blk[i]].is_empty() {
                vec![i]
            } else {
                by_blk[enc.blk[i]].clone()
            };
            if !fusion_layer || enc.blk[i] == 0 {
                sets.push(own);
            } else {
                let mut s = by_blk[0].clone();
                s.extend(own);
                sets.push(s);
            }
        }
        sets
    }

    fn attend(&self, q: &[f32], k: &[f32], v: &[f32], t: usize, sets: &[Vec<usize>]) -> Vec<f32> {
        let d = self.meta.cfg.d;
        let heads = self.meta.cfg.heads;
        let dh = d / heads;
        let scale = 1.0 / (dh as f32).sqrt();
        let mut out = vec![0f32; t * d];
        let mut sc = vec![0f64; t];
        for (i, keys) in sets.iter().enumerate().take(t) {
            for h in 0..heads {
                let qo = i * d + h * dh;
                let qr = &q[qo..qo + dh];
                let mut mx = f64::NEG_INFINITY;
                for (a, &kj) in keys.iter().enumerate() {
                    let ko = kj * d + h * dh;
                    let s = (dot(qr, &k[ko..ko + dh]) * scale) as f64;
                    sc[a] = s;
                    if s > mx {
                        mx = s;
                    }
                }
                let mut z = 0f64;
                for s in sc.iter_mut().take(keys.len()) {
                    *s = (*s - mx).exp();
                    z += *s;
                }
                let o = &mut out[qo..qo + dh];
                for (a, &kj) in keys.iter().enumerate() {
                    let p = (sc[a] / z) as f32;
                    let vo = kj * d + h * dh;
                    for (oc, &vc) in o.iter_mut().zip(&v[vo..vo + dh]) {
                        *oc += p * vc;
                    }
                }
            }
        }
        out
    }

    fn hidden(&self, enc: &Enc) -> Vec<f32> {
        let c = &self.meta.cfg;
        let (t, d, e) = (enc.ids.len(), c.d, c.emb_dim);
        let l_a = c.l_a.unwrap_or(0);
        let cont = self.continues(&enc.kind);
        let sets_f = Self::allowed_sets(enc, &cont, true);
        let sets_c = if l_a > 0 {
            Self::allowed_sets(enc, &cont, false)
        } else {
            Vec::new()
        };

        let mut raw = vec![0f32; t * e];
        for tt in 0..t {
            let id = enc.ids[tt] as usize;
            let po = enc.pos[tt] * e;
            for ch in 0..e {
                let wv = match &self.word {
                    WordTable::Full(w) => w[id * e + ch] as f64,
                    WordTable::LowRank { a, b, r } => {
                        dot64(&a[id * r..(id + 1) * r], &b[ch * r..(ch + 1) * r])
                    }
                };
                raw[tt * e + ch] = (wv + self.pos[po + ch] as f64 + self.type0[ch] as f64) as f32;
            }
        }
        let n = layernorm(&raw, t, e, &self.eln.0, Some(&self.eln.1), c.ln_eps);
        let mut x = self.proj.apply(&n, t);
        for (l, b) in self.blocks.iter().enumerate() {
            let sets = if l < l_a { &sets_c } else { &sets_f };
            let q = b.q.apply(&x, t);
            let k = b.k.apply(&x, t);
            let v = b.v.apply(&x, t);
            let y = self.attend(&q, &k, &v, t, sets);
            let mut o = b.o.apply(&y, t);
            for (oi, xi) in o.iter_mut().zip(&x) {
                *oi += xi;
            }
            x = layernorm(&o, t, d, &b.ln1.0, Some(&b.ln1.1), c.ln_eps);
            let mut f = b.fc.apply(&x, t);
            for z in f.iter_mut() {
                *z = gelu(*z);
            }
            let mut f2 = b.fc2.apply(&f, t);
            for (fi, xi) in f2.iter_mut().zip(&x) {
                *fi += xi;
            }
            x = layernorm(&f2, t, d, &b.ln2.0, Some(&b.ln2.1), c.ln_eps);
        }
        x
    }

    fn heads(&self, enc: &Enc, x: &[f32], state: &str, protos: &[Option<Protos>]) -> Vec<Answer> {
        let c = &self.meta.cfg;
        let (d, dh, t) = (c.d, c.dh_head, enc.ids.len());
        let h = &self.heads;
        let hq = layernorm(x, t, d, &h.norm.0, Some(&h.norm.1), 1e-5);
        let row = |i: usize| &hq[i * d..(i + 1) * d];
        let temp = &self.meta.temp;
        let beta = &self.meta.beta;
        let sqrt_dh = (dh as f64).sqrt();
        let has_p = h.p.is_some();
        let mut out = Vec::with_capacity(enc.qs.len());

        for (qi, q) in enc.qs.iter().enumerate() {
            let h_ans = row(q.ans);
            let pr = protos.get(qi).and_then(|p| p.as_ref());
            let pv: Option<Vec<f32>> = h.p.as_ref().map(|p| p.mv(h_ans));
            let cnt = |i: usize| pr.and_then(|p| p.cnt.get(i).copied()).unwrap_or(0);
            let pvec = |i: usize| &pr.expect("checked").vec[i * dh..(i + 1) * dh];
            // Without a stored centre, centre on the examples themselves: the count-weighted mean of
            // the prototypes, which is the centre make_protos uses when it has no running mean.
            let k_opts = if q.kind == QType::Noul {
                2
            } else {
                q.opt.len()
            };
            let derived: Option<Vec<f32>> = match pr {
                Some(p) if pv.is_some() && p.center.is_none() => {
                    let mut m = vec![0f64; dh];
                    let mut n = 0f64;
                    for i in 0..k_opts {
                        if cnt(i) > 0 {
                            n += cnt(i) as f64;
                            for (mj, &vj) in m.iter_mut().zip(pvec(i)) {
                                *mj += cnt(i) as f64 * vj as f64;
                            }
                        }
                    }
                    Some(m.iter().map(|&z| (z / n) as f32).collect())
                }
                _ => None,
            };
            // centred-cosine prototype term: beta(k) * cos(p - c, mean_k - c); None = the rule for
            // older models without a prototype projection
            let proto_term = |i: usize| -> Option<f64> {
                let Some(p) = pr else { return Some(0.0) };
                if cnt(i) <= 0 {
                    return Some(0.0);
                }
                match (&pv, p.center.as_ref().or(derived.as_ref())) {
                    (Some(pv), Some(cn)) => {
                        let v = pvec(i);
                        let a: Vec<f32> = (0..dh).map(|j| pv[j] - cn[j]).collect();
                        let b: Vec<f32> = (0..dh).map(|j| v[j] - cn[j]).collect();
                        let na = dot64(&a, &a).sqrt();
                        let nb = dot64(&b, &b).sqrt();
                        let na = if na == 0.0 { 1e-12 } else { na };
                        let nb = if nb == 0.0 { 1e-12 } else { nb };
                        Some(beta[bucket(cnt(i))] * dot64(&a, &b) / (na * nb))
                    }
                    _ => None,
                }
            };
            let lam = pr.and_then(|p| p.lam).unwrap_or(1.0);

            match q.kind {
                QType::Choice | QType::Score => {
                    let sel = if q.kind == QType::Score { 1 } else { 0 };
                    let tt = temp[if sel == 1 { 2 } else { 0 }];
                    let qa = h.a[sel].mv(h_ans);
                    let s = (h.scale[sel] as f64).exp();
                    let qv = norm64(&qa);
                    let mut z0 = Vec::with_capacity(q.opt.len());
                    let logits: Vec<f64> = q
                        .opt
                        .iter()
                        .enumerate()
                        .map(|(i, &oi)| {
                            let ov = h.o[sel].mv(row(oi));
                            let mut z = s * dot64(&qa, &ov) / sqrt_dh;
                            z0.push(z / tt);
                            match proto_term(i) {
                                Some(tv) => z += lam * tv,
                                None => {
                                    if cnt(i) > 0 {
                                        z += beta[bucket(cnt(i))] * dotd(&qv, &norm64(pvec(i)));
                                    }
                                }
                            }
                            z
                        })
                        .collect();
                    let m = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    let ex: Vec<f64> = logits.iter().map(|z| ((z - m) / tt).exp()).collect();
                    let zs: f64 = ex.iter().sum();
                    let probs: Vec<f64> = ex.iter().map(|e| e / zs).collect();
                    let k = probs.len();
                    let hh = -probs
                        .iter()
                        .map(|&p| if p > 0.0 { p * p.ln() } else { 0.0 })
                        .sum::<f64>();
                    let mut pick = 0;
                    for (i, &p) in probs.iter().enumerate() {
                        if p > probs[pick] {
                            pick = i;
                        }
                    }
                    let mut a = Answer::empty(q.kind);
                    a.confidence = Some(if k > 1 {
                        1.0 - hh / (k as f64).ln()
                    } else {
                        1.0
                    });
                    if q.kind == QType::Score {
                        a.score = Some(if k > 1 {
                            probs
                                .iter()
                                .enumerate()
                                .map(|(i, p)| p * i as f64 / (k - 1) as f64)
                                .sum()
                        } else {
                            0.0
                        });
                    }
                    a.pick = Some(pick);
                    a.probs = probs;
                    a.qvec = match &pv {
                        Some(pv) => pv.clone(),
                        None => qv.iter().map(|&z| z as f32).collect(),
                    };
                    a.proj = has_p;
                    a.z0 = z0;
                    out.push(a);
                }
                QType::Noul => {
                    let mut z = dot64(&h.noul_w, h_ans) + h.noul_b as f64;
                    let z0 = z / temp[1];
                    let nv = norm64(&h.noul_q.mv(h_ans));
                    if pr.is_some() {
                        let term: Vec<f64> = (0..2)
                            .map(|i| match proto_term(i) {
                                Some(tv) => lam * tv,
                                None => {
                                    if cnt(i) > 0 {
                                        beta[bucket(cnt(i))] * dotd(&nv, &norm64(pvec(i)))
                                    } else {
                                        0.0
                                    }
                                }
                            })
                            .collect();
                        z += term[1] - term[0];
                    }
                    let mut a = Answer::empty(QType::Noul);
                    a.p = Some(1.0 / (1.0 + (-z / temp[1]).exp()));
                    a.qvec = match &pv {
                        Some(pv) => pv.clone(),
                        None => nv.iter().map(|&z| z as f32).collect(),
                    };
                    a.proj = has_p;
                    a.z0 = vec![0.0, z0];
                    out.push(a);
                }
                QType::Span => {
                    let s = enc.st_idx.len();
                    let tt = temp[3];
                    let qs = h.sq.mv(h_ans);
                    let mut start = Vec::with_capacity(s + 1);
                    start.push(dot64(&h.snull_w, h_ans) + h.snull_b as f64);
                    for &i in &enc.st_idx {
                        start.push(dot64(&qs, &h.sk.mv(row(i))) / sqrt_dh);
                    }
                    let ls = lsm(&start, tt);
                    let mut best = 0;
                    for t in 1..s {
                        if ls[1 + t] > ls[1 + best] {
                            best = t;
                        }
                    }
                    let eq = h.eq.mv(h_ans);
                    let es = h.es.mv(row(enc.st_idx.get(best).copied().unwrap_or(0)));
                    let qe: Vec<f32> = eq.iter().zip(&es).map(|(a, b)| a + b).collect();
                    let span_max = self.meta.format.span_max;
                    let e: Vec<f64> = enc
                        .st_idx
                        .iter()
                        .enumerate()
                        .map(|(t, &i)| {
                            if t >= best && t < best + span_max {
                                dot64(&qe, &h.ek.mv(row(i))) / sqrt_dh
                            } else {
                                -1e4
                            }
                        })
                        .collect();
                    let le = if s > 0 { lsm(&e, tt) } else { Vec::new() };
                    let mut best_e = best;
                    for t in 0..s {
                        if le[t] > le[best_e] {
                            best_e = t;
                        }
                    }
                    let mut a = Answer::empty(QType::Span);
                    a.p_present = Some(1.0 - ls[0].exp());
                    a.tok = Some((best, best_e));
                    a.p_span = Some(if s > 0 {
                        (ls[1 + best] + le[best_e]).exp()
                    } else {
                        0.0
                    });
                    let mut text = String::new();
                    if s > 0 {
                        if let (Some(&(a0, _)), Some(&(_, b1))) =
                            (enc.st_off.get(best), enc.st_off.get(best_e))
                        {
                            a.char = Some((a0, b1));
                            text = state.get(a0..b1).map(js_trim).unwrap_or("").to_string();
                        }
                    }
                    a.text = Some(text);
                    out.push(a);
                }
            }
        }
        out
    }
}
