//! meta.json and model.bin: tensor table, dequantisation (Q4_0 block-32 with bf16 scales, int8 rows
//! with one f32 scale each, plain f32).

use std::collections::HashMap;

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub name: String,
    pub cfg: Cfg,
    pub tensors: Vec<TensorMeta>,
    pub specials: HashMap<String, u32>,
    pub format: Format,
    pub temp: Vec<f64>,
    pub beta: Vec<f64>,
    pub tokenizer: TokMeta,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cfg {
    pub arch: String,
    pub d: usize,
    pub layers: usize,
    #[serde(rename = "L_a", default)]
    pub l_a: Option<usize>,
    #[serde(default)]
    pub fusion: Option<String>,
    pub dh_head: usize,
    pub heads: usize,
    pub ln_eps: f64,
    pub emb_dim: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Format {
    pub ts_max: usize,
    pub p_q: usize,
    pub q_max: usize,
    #[serde(default)]
    pub k_max: Option<usize>,
    pub span_max: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokMeta {
    pub kind: String,
    pub vocab: Vec<Option<String>>,
    pub unk: String,
    pub prefix: String,
    pub max_chars: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TensorMeta {
    pub name: String,
    pub shape: Vec<usize>,
    pub dtype: String,
    pub offset: usize,
    #[serde(default)]
    pub scale_offset: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Tensor {
    pub data: Vec<f32>,
    pub shape: Vec<usize>,
}

fn slice<'a>(buf: &'a [u8], off: usize, len: usize, name: &str) -> Result<&'a [u8]> {
    off.checked_add(len)
        .filter(|&e| e <= buf.len())
        .map(|e| &buf[off..e])
        .ok_or_else(|| Error::Model(format!("tensor {name} runs past the end of model.bin")))
}

fn f32_at(b: &[u8], i: usize) -> f32 {
    f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
}

pub fn dequant(e: &TensorMeta, buf: &[u8]) -> Result<Tensor> {
    let name = &e.name;
    // every dtype stores at least half a byte per value, so this also bounds the sizes below
    let n = e
        .shape
        .iter()
        .try_fold(1usize, |a, &b| a.checked_mul(b))
        .filter(|&n| n / 2 <= buf.len())
        .ok_or_else(|| Error::Model(format!("tensor {name} is larger than model.bin")))?;
    let need_scale = || e.scale_offset.ok_or_else(|| Error::Model(format!("tensor {name} has no scale_offset")));
    let data = match e.dtype.as_str() {
        "f32" => {
            let b = slice(buf, e.offset, 4 * n, name)?;
            (0..n).map(|i| f32_at(b, i)).collect()
        }
        "q4" => {
            if e.shape.len() != 2 || e.shape[1] % 32 != 0 {
                return Err(Error::Model(format!("q4 tensor {name} must be 2-D with columns a multiple of 32")));
            }
            let (rows, cols) = (e.shape[0], e.shape[1]);
            let nb = cols / 32;
            let nib = slice(buf, e.offset, rows * nb * 16, name)?;
            let sc = slice(buf, need_scale()?, rows * nb * 2, name)?;
            let mut arr = vec![0f32; n];
            for r in 0..rows {
                for b in 0..nb {
                    let i = r * nb + b;
                    let bits = (u16::from_le_bytes([sc[2 * i], sc[2 * i + 1]]) as u32) << 16;
                    let d = f32::from_bits(bits);
                    let (no, wo) = (i * 16, r * cols + b * 32);
                    for k in 0..16 {
                        let byte = nib[no + k];
                        arr[wo + k] = ((byte & 15) as i32 - 8) as f32 * d;
                        arr[wo + k + 16] = ((byte >> 4) as i32 - 8) as f32 * d;
                    }
                }
            }
            arr
        }
        "int8" => {
            if e.shape.len() != 2 {
                return Err(Error::Model(format!("int8 tensor {name} must be 2-D")));
            }
            let (rows, cols) = (e.shape[0], e.shape[1]);
            let q = slice(buf, e.offset, n, name)?;
            let s = slice(buf, need_scale()?, 4 * rows, name)?;
            let mut arr = vec![0f32; n];
            for r in 0..rows {
                let sc = f32_at(s, r);
                for c in 0..cols {
                    arr[r * cols + c] = (q[r * cols + c] as i8) as f32 * sc;
                }
            }
            arr
        }
        other => return Err(Error::Model(format!("tensor {name} has unknown dtype {other}"))),
    };
    Ok(Tensor { data, shape: e.shape.clone() })
}

pub struct Weights {
    t: HashMap<String, Tensor>,
}

impl Weights {
    pub fn new(meta: &Meta, buf: &[u8]) -> Result<Self> {
        let mut t = HashMap::with_capacity(meta.tensors.len());
        for e in &meta.tensors {
            t.insert(e.name.clone(), dequant(e, buf)?);
        }
        Ok(Weights { t })
    }

    pub fn has(&self, n: &str) -> bool {
        self.t.contains_key(n)
    }

    /// Moves a tensor out, checking its shape (`None` entries match anything).
    pub fn take(&mut self, n: &str, shape: &[Option<usize>]) -> Result<Tensor> {
        let x = self.t.remove(n).ok_or_else(|| Error::Model(format!("missing tensor {n}")))?;
        let ok = x.shape.len() == shape.len() && x.shape.iter().zip(shape).all(|(a, b)| b.is_none_or(|b| *a == b));
        if !ok {
            return Err(Error::Model(format!("tensor {n} has shape {:?}, expected {:?}", x.shape, shape)));
        }
        Ok(x)
    }
}
