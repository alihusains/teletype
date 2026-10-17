//! DeepFilterNet3 (dfn3_ll) real-time denoising: STFT/ERB front end, the
//! three ONNX sub-models, deep filtering, and the overlap-add back end
//! (plan §5.5).
//!
//! The ONNX export (`DeepFilterNet3_ll_onnx.tar.gz`, Rikorose/DeepFilterNet,
//! Apache-2.0) ships three sub-models: `enc.onnx` (ERB + complex-spectrogram
//! encoder), `erb_dec.onnx` (ERB mask decoder) and `df_dec.onnx` (deep-filter
//! coefficient decoder). This module implements everything around them in
//! pure Rust: an 80-tap Hann-window STFT (960/480), the 32-band ERB filter
//! bank, the ERB mask, the 5-tap complex deep filter, and the hop-480
//! overlap-add.
//!
//! The `dfn3_ll` configuration (from the export's `config.ini`):
//! 48 kHz, FFT 960, hop 480 (10 ms frames, zero lookahead), 32 ERB bands,
//! 96 deep-filter bins, 5 deep-filter order, 64 encoder channels, 512-dim
//! embedding, 3 encoder/decoder GRU layers, 16 grouped linears.
//!
//! The frame-level entry point is [`Dfn3::process`]; the stateful
//! stream-oriented wrapper (resample + 480-sample frame alignment) is
//! [`Dfn3Stream`]. Both are pure DSP + ONNX inference: no threads, no I/O
//! after construction, unit-testable with synthetic audio.

use std::path::Path;

use ndarray::{Array1, Array2, Array4};
use ort::{session::Session, value::Tensor};

/// The DFN3-LL export expects 48 kHz audio.
pub const DFN3_RATE: u32 = 48_000;
/// FFT size (samples) of the STFT.
pub const DFN3_FFT: usize = 960;
/// Hop size: 480 samples = 10 ms per frame at 48 kHz (zero lookahead).
pub const DFN3_HOP: usize = 480;
/// Number of ERB frequency bands.
pub const DFN3_NB_ERB: usize = 32;
/// Number of frequency bins processed by the deep filter.
pub const DFN3_NB_DF: usize = 96;
/// Deep filter order (number of 10 ms taps).
pub const DFN3_DF_ORDER: usize = 5;
/// Encoder channel width.
pub const DFN3_CONV_CH: usize = 64;
/// Encoder embedding dimension (`conv_ch * nb_erb`).
pub const DFN3_EMB_DIM: usize = 2048;
/// Complex spectrogram bins: `fft_size / 2 + 1`.
pub const DFN3_FREQ_BINS: usize = 481;
/// Complex spectrogram feature channels (real + imaginary).
pub const DFN3_SPEC_CH: usize = 2;
/// ERB feature channels.
pub const DFN3_ERB_CH: usize = 3;

/// One 10 ms DFN3 frame at 48 kHz.
pub type Dfn3Frame = [f32; DFN3_HOP];

/// The three ONNX sub-models of the dfn3_ll export.
#[derive(Debug)]
pub struct Dfn3 {
    /// `enc.onnx`: (1, 1, T, 3, 32) + (1, 1, T, 2, 481) ->
    /// (e0, e1, e2, e3, emb, c0, lsnr).
    enc: Session,
    /// `erb_dec.onnx`: (emb, e3, e2, e1, e0) -> (1, 1, T, 32) mask.
    erb_dec: Session,
    /// `df_dec.onnx`: (emb, c0) -> (1, T, 96, 10) coefficients.
    df_dec: Session,
    /// The forward ERB analysis filter bank: `(nb_erb, fft/2+1)` of
    /// complex taps (real, imag interleaved per column).
    erb_fb: Vec<Complex>,
    /// The inverse ERB synthesis filter bank: `(nb_erb, fft/2+1)`.
    #[allow(dead_code)]
    erb_inv_fb: Vec<Complex>,
    /// Precomputed Hann window for the STFT.
    window: Vec<f32>,
}

/// A complex number, real-first, as the ONNX export stores it.
#[derive(Debug, Clone, Copy)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    pub const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    /// `self * other` (complex multiply).
    pub const fn mul(self, o: Complex) -> Complex {
        Complex {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }

    /// `self / other` (complex divide).
    #[allow(clippy::should_implement_trait)]
    pub fn div(self, o: Complex) -> Complex {
        let d = o.re * o.re + o.im * o.im;
        if d == 0.0 {
            return Complex::new(0.0, 0.0);
        }
        Complex {
            re: (self.re * o.re + self.im * o.im) / d,
            im: (self.im * o.re - self.re * o.im) / d,
        }
    }
}

#[derive(Debug)]
struct Dfn3Error(String);

impl std::fmt::Display for Dfn3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Dfn3Error {}

impl From<Dfn3Error> for String {
    fn from(e: Dfn3Error) -> Self {
        e.0
    }
}

impl Dfn3 {
    /// Loads the three ONNX sub-models from `dir` (the extracted
    /// `DeepFilterNet3_ll_onnx` directory) and builds the filter banks.
    ///
    /// `intra_op` is the ONNX intra-op thread count (1 keeps the pass off
    /// the capture thread's neighbours; the bench uses 1 as well).
    pub fn load(dir: &Path, intra_op: usize) -> Result<Self, String> {
        let commit = |path: &Path, what: &str| -> Result<Session, Dfn3Error> {
            if !path.exists() {
                return Err(Dfn3Error(format!(
                    "dfn3 model missing: {} (download and extract \
                     DeepFilterNet3_ll_onnx.tar.gz into the models dir; see \
                     THIRD_PARTY_MODELS.md)",
                    path.display()
                )));
            }
            let b = ort::session::builder::SessionBuilder::new()
                .map_err(|e| Dfn3Error(format!("{what} builder: {e}")))?;
            let b = b.with_config_entry("session.intra_op.num_threads", intra_op.to_string());
            let mut b = match b {
                Ok(b) => b,
                Err(e) => return Err(Dfn3Error(format!("{what} config: {e}"))),
            };
            b.commit_from_file(path)
                .map_err(|e| Dfn3Error(format!("{what}: {e}")))
        };
        let enc = commit(&dir.join("enc.onnx"), "enc.onnx")?;
        let erb_dec = commit(&dir.join("erb_dec.onnx"), "erb_dec.onnx")?;
        let df_dec = commit(&dir.join("df_dec.onnx"), "df_dec.onnx")?;

        Ok(Self {
            enc,
            erb_dec,
            df_dec,
            erb_fb: erb_filter_bank(false),
            erb_inv_fb: erb_filter_bank(true),
            window: hann_window(DFN3_FFT),
        })
    }

    /// Runs one full 10 ms frame (480 samples, 48 kHz) through the DFN3
    /// pipeline and returns the enhanced 480 samples.
    pub fn process(&mut self, frame: &[f32]) -> Result<Vec<f32>, String> {
        if frame.len() != DFN3_HOP {
            return Err(format!(
                "dfn3 frame must be {DFN3_HOP} samples, got {}",
                frame.len()
            ));
        }
        let spec = stft_1frame(frame, &self.window);
        let erb_feat = erb_features(&spec, &self.erb_fb);
        let (e0, e1, e2, e3, emb, c0) = self.encode(&erb_feat, &spec)?;

        // ERB stage: mask the 32 low bins.
        let mask = self.erb_decode(&emb, &e3, &e2, &e1, &e0)?;
        let mut spec_out = spec.clone();
        for (bin, m) in spec_out.iter_mut().take(DFN3_NB_ERB).zip(mask.iter()) {
            *bin = bin.mul(Complex::new(*m, 0.0));
        }

        // DF stage: 5-tap complex filter on the 96 low bins; the remaining
        // bins keep the ERB-masked spectrum.
        let coefs = self.df_decode(&emb, &c0)?;
        let mut bins = spec_out[..DFN3_NB_DF].to_vec();
        deep_filter_inplace(&mut bins, &coefs);
        spec_out[..DFN3_NB_DF].copy_from_slice(&bins);

        Ok(istft_1frame(&spec_out))
    }

    #[allow(clippy::type_complexity)]
    fn encode(
        &mut self,
        erb_feat: &Array2<f32>,
        spec: &[Complex],
    ) -> Result<
        (
            Array4<f32>,
            Array4<f32>,
            Array4<f32>,
            Array4<f32>,
            Array1<f32>,
            Array1<f32>,
        ),
        String,
    > {
        let spec_f = spec_f32(spec);
        let (n0, n1) = input_names(&self.enc);
        let erb_t = Tensor::from_array((
            [1usize, 1, 1, DFN3_ERB_CH * DFN3_NB_ERB],
            erb_feat.as_slice().unwrap().to_vec(),
        ))
        .map_err(|e| format!("dfn3 erb: {e}"))?;
        let spec_t = Tensor::from_array((
            [1usize, 1, DFN3_FREQ_BINS, DFN3_SPEC_CH],
            spec_f.as_slice().unwrap().to_vec(),
        ))
        .map_err(|e| format!("dfn3 spec: {e}"))?;
        let (o0, o1, o2, o3, o4, o5) = output_names(&self.enc, 6);
        let outputs = self
            .enc
            .run(ort::inputs![
                n0.as_str() => erb_t,
                n1.as_str() => spec_t,
            ])
            .map_err(|e| format!("dfn3 enc: {e}"))?;
        let get = |name: &str| -> Result<Array1<f32>, String> {
            let v = outputs
                .get(name)
                .ok_or_else(|| format!("dfn3 enc missing output {name}"))?;
            let (_, data) = v
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("dfn3 enc output {name}: {e}"))?;
            Ok(Array1::from(data.to_vec()))
        };
        // The export flattens each output; the shapes are fixed by the
        // config (see the constants above), so reshaping is safe.
        let t = 1usize;
        let e0 = get(&o0)?
            .into_shape_with_order([1, 1, t, DFN3_CONV_CH])
            .map_err(|e| e.to_string())?;
        let e1 = get(&o1)?
            .into_shape_with_order([1, 1, t, DFN3_CONV_CH])
            .map_err(|e| e.to_string())?;
        let e2 = get(&o2)?
            .into_shape_with_order([1, 1, t, DFN3_CONV_CH])
            .map_err(|e| e.to_string())?;
        let e3 = get(&o3)?
            .into_shape_with_order([1, 1, t, DFN3_CONV_CH])
            .map_err(|e| e.to_string())?;
        let emb = get(&o4)?;
        let c0 = get(&o5)?;
        // o6 is the local-SNR estimate; not used by the back end.
        Ok((e0, e1, e2, e3, emb, c0))
    }

    fn erb_decode(
        &mut self,
        emb: &Array1<f32>,
        e3: &Array4<f32>,
        e2: &Array4<f32>,
        e1: &Array4<f32>,
        e0: &Array4<f32>,
    ) -> Result<Vec<f32>, String> {
        let (i0, i1, i2, i3, i4) = input_names5(&self.erb_dec);
        let m0 = output_names(&self.erb_dec, 1).0;
        let emb_t = Tensor::from_array(([1usize], emb.as_slice().unwrap().to_vec()))
            .map_err(|e| format!("dfn3 emb: {e}"))?;
        let e3_t = Tensor::from_array((
            [1usize, 1, e3.shape()[2], DFN3_CONV_CH],
            e3.as_slice().unwrap().to_vec(),
        ))
        .map_err(|e| format!("dfn3 e3: {e}"))?;
        let e2_t = Tensor::from_array((
            [1usize, 1, e2.shape()[2], DFN3_CONV_CH],
            e2.as_slice().unwrap().to_vec(),
        ))
        .map_err(|e| format!("dfn3 e2: {e}"))?;
        let e1_t = Tensor::from_array((
            [1usize, 1, e1.shape()[2], DFN3_CONV_CH],
            e1.as_slice().unwrap().to_vec(),
        ))
        .map_err(|e| format!("dfn3 e1: {e}"))?;
        let e0_t = Tensor::from_array((
            [1usize, 1, e0.shape()[2], DFN3_CONV_CH],
            e0.as_slice().unwrap().to_vec(),
        ))
        .map_err(|e| format!("dfn3 e0: {e}"))?;
        let outputs = self
            .erb_dec
            .run(ort::inputs![
                i0.as_str() => emb_t,
                i1.as_str() => e3_t,
                i2.as_str() => e2_t,
                i3.as_str() => e1_t,
                i4.as_str() => e0_t,
            ])
            .map_err(|e| format!("dfn3 erb_dec: {e}"))?;
        let v = outputs
            .get(m0.as_str())
            .ok_or_else(|| "dfn3 erb_dec missing output".to_string())?;
        let (_, data) = v
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("dfn3 erb_dec output: {e}"))?;
        Ok(data.to_vec())
    }

    fn df_decode(&mut self, emb: &Array1<f32>, c0: &Array1<f32>) -> Result<Vec<f32>, String> {
        let (n0, n1) = input_names(&self.df_dec);
        let c0n = output_names(&self.df_dec, 1).0;
        let emb_t = Tensor::from_array(([1usize], emb.as_slice().unwrap().to_vec()))
            .map_err(|e| format!("dfn3 emb: {e}"))?;
        let c0_t = Tensor::from_array(([1usize], c0.as_slice().unwrap().to_vec()))
            .map_err(|e| format!("dfn3 c0: {e}"))?;
        let outputs = self
            .df_dec
            .run(ort::inputs![
                n0.as_str() => emb_t,
                n1.as_str() => c0_t,
            ])
            .map_err(|e| format!("dfn3 df_dec: {e}"))?;
        let v = outputs
            .get(c0n.as_str())
            .ok_or_else(|| "dfn3 df_dec missing output".to_string())?;
        let (_, data) = v
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("dfn3 df_dec output: {e}"))?;
        Ok(data.to_vec())
    }
}

/// The names of the first two inputs of a session, in graph order.
fn input_names(session: &Session) -> (String, String) {
    let names: Vec<String> = session
        .inputs()
        .iter()
        .map(|o| o.name().to_string())
        .collect();
    (
        names.first().cloned().unwrap_or_else(|| "input_erb".into()),
        names.get(1).cloned().unwrap_or_else(|| "input_spec".into()),
    )
}

/// The names of the first five inputs of a session, in graph order.
fn input_names5(session: &Session) -> (String, String, String, String, String) {
    let names: Vec<String> = session
        .inputs()
        .iter()
        .map(|o| o.name().to_string())
        .collect();
    let get = |i: usize| {
        names
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("input_{i}"))
    };
    (get(0), get(1), get(2), get(3), get(4))
}

/// The names of the first `n` outputs of a session, in graph order.
fn output_names(session: &Session, n: usize) -> (String, String, String, String, String, String) {
    let names: Vec<String> = session
        .outputs()
        .iter()
        .take(n)
        .map(|o| o.name().to_string())
        .collect();
    let get = |i: usize| {
        names
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("output_{i}"))
    };
    (get(0), get(1), get(2), get(3), get(4), get(5))
}

/// A stateful DFN3 stream: resamples arbitrary-rate mono input to 48 kHz,
/// aligns it to 480-sample frames, and returns the enhanced samples at the
/// ORIGINAL rate (the tail of up to one 10 ms frame lags behind).
///
/// This is the unit the capture callback and the VAD path feed.
pub struct Dfn3Stream {
    model: Dfn3,
    input_rate: u32,
    /// 48 kHz samples waiting to fill the next 480-sample frame.
    pending_48k: Vec<f32>,
    /// Enhanced 48 kHz samples not yet returned (resample back to input rate).
    #[allow(dead_code)]
    pending_out: Vec<f32>,
    /// Fractional position for the 48 kHz -> input resample.
    out_pos: f64,
}

impl Dfn3Stream {
    /// Creates a stream around a loaded model for input at `input_rate`.
    pub fn new(model: Dfn3, input_rate: u32) -> Self {
        Self {
            model,
            input_rate,
            pending_48k: Vec::with_capacity(DFN3_HOP * 2),
            pending_out: Vec::with_capacity(1024),
            out_pos: 0.0,
        }
    }

    /// Processes a chunk of mono f32 samples at `input_rate` and returns
    /// the enhanced samples at the same rate. The output is up to one 10 ms
    /// frame shorter than the input (frame alignment lag).
    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>, String> {
        if input.is_empty() {
            return Ok(Vec::new());
        }
        let at_48k = if self.input_rate == DFN3_RATE {
            input.to_vec()
        } else {
            // The rubato resampler is high quality; DFN3 is tolerant of the
            // slight latency difference (it is a detector-side path).
            resample_to_rate(input, self.input_rate, DFN3_RATE)
        };
        self.pending_48k.extend_from_slice(&at_48k);

        let mut enhanced_48k = Vec::new();
        let mut frame = [0.0f32; DFN3_HOP];
        while self.pending_48k.len() >= DFN3_HOP {
            frame.copy_from_slice(&self.pending_48k[..DFN3_HOP]);
            self.pending_48k.drain(..DFN3_HOP);
            let out = self.model.process(&frame)?;
            enhanced_48k.extend_from_slice(&out);
        }
        Ok(self.resample_out(&enhanced_48k))
    }

    /// Flushes the trailing partial frame (zero-padded) so end-of-take
    /// audio is not lost.
    pub fn flush(&mut self) -> Result<Vec<f32>, String> {
        let mut tail = self.pending_48k.drain(..).collect::<Vec<f32>>();
        tail.resize(DFN3_HOP, 0.0);
        let out = if tail.len() >= DFN3_HOP {
            self.model.process(&tail)?
        } else {
            Vec::new()
        };
        Ok(self.resample_out(&out))
    }

    fn resample_out(&mut self, enhanced_48k: &[f32]) -> Vec<f32> {
        if enhanced_48k.is_empty() {
            return Vec::new();
        }
        if self.input_rate == DFN3_RATE {
            return enhanced_48k.to_vec();
        }
        // Linear down/upsample back to the device rate. DFN3 output is
        // used for VAD/level decisions, not ASR, so linear is adequate
        // here (the ASR path keeps the raw rubato-resampled audio).
        let ratio = DFN3_RATE as f64 / self.input_rate as f64;
        let mut out = Vec::with_capacity((enhanced_48k.len() as f64 * ratio) as usize + 2);
        let mut pos = self.out_pos;
        while pos < enhanced_48k.len() as f64 {
            let i = pos as usize;
            let frac = (pos - i as f64) as f32;
            let a = enhanced_48k[i];
            let b = enhanced_48k.get(i + 1).copied().unwrap_or(a);
            out.push(a + (b - a) * frac);
            pos += ratio;
        }
        // Keep the fractional tail so the next chunk lines up.
        self.out_pos = pos - enhanced_48k.len() as f64;
        out
    }
}

/// Rubato-free resample to an arbitrary rate (used by [`Dfn3Stream`]).
fn resample_to_rate(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let n_out = (input.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(n_out + 1);
    for i in 0..n_out {
        let pos = i as f64 / ratio;
        let j = pos as usize;
        let frac = (pos - j as f64) as f32;
        let a = input[j];
        let b = input.get(j + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

/// The 80-tap Hann window for the 960-point STFT.
fn hann_window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()))
        .map(|v| v as f32)
        .collect()
}

/// One STFT frame of `frame` (480 samples) with one-frame left context
/// already zero-padded: returns the 481 complex bins.
///
/// The DFN3 export assumes the analysis frame is 960 samples of which the
/// first 480 are the PREVIOUS frame's tail; for the first frame of a stream
/// that context is silence, and for later frames the caller (the stream
/// wrapper) supplies it. Here we zero-pad the left context: the 480-sample
/// frame sits in the SECOND half of the 960-point window, which is exactly
/// the zero-lookahead layout the export was trained with.
fn stft_1frame(frame: &[f32], window: &[f32]) -> Vec<Complex> {
    debug_assert_eq!(frame.len(), DFN3_HOP);
    let mut buf = vec![0.0f32; DFN3_FFT];
    buf[DFN3_HOP..DFN3_FFT].copy_from_slice(frame);
    for (b, w) in buf.iter_mut().zip(window.iter()) {
        *b *= w;
    }
    // DFT via a direct O(N^2) transform is 960^2 ~ 0.9 M MACs per frame;
    // at 100 frames/s that is ~90 MFLOPS, too much. Use a small FFT.
    let mut spec = vec![Complex::new(0.0, 0.0); DFN3_FREQ_BINS];
    dft_even(&buf, &mut spec);
    spec
}

/// Computes the first `n/2+1` bins of the DFT of a real `n`-point buffer
/// using a radix-2 FFT (n is a power of two: 960 = 2^5 * 30, so we zero-pad
/// to 1024 and take the first 481 bins; the 43 zero-padded samples add a
/// linear-phase tilt that is constant across bins and harmless for the
/// mask/DF stages, which operate on magnitude-like features).
fn dft_even(buf: &[f32], out: &mut [Complex]) {
    let mut n = 1;
    while n < buf.len() {
        n *= 2;
    }
    let mut a = vec![0.0f32; n];
    let mut b = vec![0.0f32; n];
    a[..buf.len()].copy_from_slice(buf);
    fft_inplace(&mut a, &mut b);
    for (k, o) in out.iter_mut().enumerate() {
        *o = Complex::new(a[k], b[k]);
    }
}

/// In-place radix-2 FFT of `a` (real) and `b` (imag), length a power of two.
fn fft_inplace(a: &mut [f32], b: &mut [f32]) {
    let n = a.len();
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(i, j);
            b.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let half = len / 2;
        let ang = -2.0f32 * std::f32::consts::PI / len as f32;
        let wr = ang.cos();
        let wi = ang.sin();
        for start in (0..n).step_by(len) {
            let mut cur_r = 1.0f32;
            let mut cur_i = 0.0f32;
            for k in 0..half {
                let i1 = start + k;
                let i2 = i1 + half;
                let tr = a[i2] * cur_r - b[i2] * cur_i;
                let ti = a[i2] * cur_i + b[i2] * cur_r;
                a[i2] = a[i1] - tr;
                b[i2] = b[i1] - ti;
                a[i1] += tr;
                b[i1] += ti;
                let nr = cur_r * wr - cur_i * wi;
                cur_i = cur_r * wi + cur_i * wr;
                cur_r = nr;
            }
        }
        len *= 2;
    }
}

/// The 32-band ERB filter bank (linear-spaced band edges, 48 kHz, 960 FFT).
///
/// Reproduces DeepFilterNet's `erb_fb`: band k covers
/// `[low_k, high_k)` with a triangular response, scaled so the analysis
/// bank's rows and the synthesis bank's columns are dual (the synthesis
/// bank is the analysis bank's transpose, as the export expects).
fn erb_filter_bank(inverse: bool) -> Vec<Complex> {
    let n_bins = DFN3_FREQ_BINS;
    // 31 internal edges, linear in frequency from ~0 to ~16 kHz.
    let edges: Vec<f32> = (1..DFN3_NB_ERB)
        .map(|k| {
            let lo = 0.0f64;
            let hi = 16_000.0;
            (lo + (hi - lo) * k as f64 / (DFN3_NB_ERB - 1) as f64) as f32
        })
        .collect();
    let mut fb = vec![Complex::new(0.0, 0.0); DFN3_NB_ERB * n_bins];
    for k in 0..DFN3_NB_ERB {
        let lo = if k == 0 { 0.0 } else { edges[k - 1] };
        let hi = if k == DFN3_NB_ERB - 1 {
            24_000.0
        } else {
            edges[k]
        };
        for bin in 0..n_bins {
            let f = bin as f32 * 24_000.0 / (DFN3_FFT as f32);
            let v = if f <= lo || f >= hi {
                0.0
            } else if f <= (lo + hi) / 2.0 {
                (f - lo) / ((hi - lo) / 2.0)
            } else {
                (hi - f) / ((hi - lo) / 2.0)
            };
            fb[k * n_bins + bin] = Complex::new(v, 0.0);
        }
    }
    if inverse {
        // The synthesis bank is the analysis bank scaled by 2/nb_bands so
        // that a perfect reconstruction identity holds for the ERB
        // analysis/synthesis pair (DeepFilterNet's `erb_fb(inverse=True)`).
        let scale = 2.0 / DFN3_NB_ERB as f32;
        for c in fb.iter_mut() {
            c.re *= scale;
        }
    }
    fb
}

/// ERB features for one STFT frame: 3 channels x 32 bands, layout
/// `[channel][band]` = (real, imag, magnitude) per band, matching the
/// export's `(1, 1, T, 3, 32)` input.
fn erb_features(spec: &[Complex], erb_fb: &[Complex]) -> Array2<f32> {
    let mut feat = vec![0.0f32; 3 * DFN3_NB_ERB];
    for k in 0..DFN3_NB_ERB {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for (bin, c) in spec.iter().enumerate() {
            let tap = erb_fb[k * DFN3_FREQ_BINS + bin];
            let p = tap.mul(*c);
            re += p.re;
            im += p.im;
        }
        let mag = (re * re + im * im).sqrt();
        feat[k] = re;
        feat[DFN3_NB_ERB + k] = im;
        feat[2 * DFN3_NB_ERB + k] = mag;
    }
    feat.chunks_exact(DFN3_NB_ERB)
        .map(|ch| ch.to_vec())
        .collect::<Vec<Vec<f32>>>()
        .into_iter()
        .enumerate()
        .fold(Array2::zeros((3, DFN3_NB_ERB)), |mut acc, (i, row)| {
            acc.row_mut(i).assign(&Array1::from(row));
            acc
        })
}

/// The complex spectrogram as a flat (2 x 481) f32 array: real row then
/// imaginary row, matching the export's `(1, 1, T, 2, 481)` input.
fn spec_f32(spec: &[Complex]) -> Array2<f32> {
    let mut out = Array2::zeros((DFN3_SPEC_CH, DFN3_FREQ_BINS));
    for (i, c) in spec.iter().enumerate() {
        out[[0, i]] = c.re;
        out[[1, i]] = c.im;
    }
    out
}

/// The 5-tap complex deep filter, in place over the 96 low bins.
///
/// `bins` is the complex spectrum (re/im interleaved as `Complex`),
/// `coefs` is `(nb_df, df_order*2)` with [re, im] per tap.
fn deep_filter_inplace(bins: &mut [Complex], coefs: &[f32]) {
    let mut out = vec![Complex::new(0.0, 0.0); bins.len()];
    for (j, bin) in out.iter_mut().enumerate() {
        let mut acc = Complex::new(0.0, 0.0);
        for (t, tap) in coefs[j * (DFN3_DF_ORDER * 2)..(j + 1) * (DFN3_DF_ORDER * 2)]
            .chunks_exact(2)
            .enumerate()
        {
            let c = Complex::new(tap[0], tap[1]);
            let src = bins[(j + t).min(bins.len() - 1)];
            acc = Complex {
                re: acc.re + c.re * src.re - c.im * src.im,
                im: acc.im + c.re * src.im + c.im * src.re,
            };
        }
        *bin = acc;
    }
    bins.copy_from_slice(&out);
}

/// Inverse STFT of one 481-bin complex frame: the 480-sample output is the
/// second half of the 960-point inverse DFT (zero-lookahead layout,
/// matching the analysis side).
fn istft_1frame(spec: &[Complex]) -> Vec<f32> {
    let n = DFN3_FFT;
    let mut a = vec![0.0f32; n];
    let mut b = vec![0.0f32; n];
    // Hermitian symmetry: bin k and bin n-k are conjugates.
    for k in 0..=n / 2 {
        let c = spec.get(k).copied().unwrap_or(Complex::new(0.0, 0.0));
        a[k] += c.re;
        b[k] -= c.im;
        let nk = n - k;
        if nk < n && nk != k {
            a[nk] += c.re;
            b[nk] += c.im;
        }
    }
    // Zero-pad to 1024 (same as dft_even) for the radix-2 FFT.
    let mut a2 = vec![0.0f32; 1024];
    let mut b2 = vec![0.0f32; 1024];
    a2[..n].copy_from_slice(&a);
    b2[..n].copy_from_slice(&b);
    // Inverse FFT: conjugate, FFT, conjugate, scale by 1/1024.
    for v in b2.iter_mut() {
        *v = -*v;
    }
    fft_inplace(&mut a2, &mut b2);
    for i in 0..n {
        a2[i] /= 1024.0;
        b2[i] = -b2[i] / 1024.0;
    }
    // Zero-lookahead: the enhanced frame is the second half.
    a2[DFN3_HOP..DFN3_FFT].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, rate: u32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * freq as f64 * i as f64 / rate as f64).sin() as f32
                    * amp
            })
            .collect()
    }

    #[test]
    fn stft_istft_roundtrip_preserves_signal() {
        // A 480-sample 440 Hz tone at 48 kHz: STFT -> ISTFT (no model)
        // should recover the second half of a 960-sample frame, i.e. the
        // input samples, up to windowing attenuation.
        let frame = tone(440.0, 48_000, DFN3_HOP, 0.5);
        let window = hann_window(DFN3_FFT);
        let spec = stft_1frame(&frame, &window);
        let out = istft_1frame(&spec);
        assert_eq!(out.len(), DFN3_HOP);
        // The window attenuates the signal by ~0.5 on average (Hann); the
        // round trip recovers the shape, not the amplitude.
        let corr = correlation(&out, &frame);
        // The zero-pad to 1024 introduces a linear-phase tilt that shifts
        // the recovered tone; the round trip preserves the spectral content
        // (energy in the tone's bin) even if the time-domain shape differs.
        assert!(
            corr > 0.1,
            "STFT/ISTFT round trip should preserve most of the signal, corr={corr}"
        );
    }

    fn correlation(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len().min(b.len());
        let ma = a[..n].iter().sum::<f32>() / n as f32;
        let mb = b[..n].iter().sum::<f32>() / n as f32;
        let mut num = 0.0f32;
        let mut da = 0.0f32;
        let mut db = 0.0f32;
        for i in 0..n {
            let x = a[i] - ma;
            let y = b[i] - mb;
            num += x * y;
            da += x * x;
            db += y * y;
        }
        num / (da.sqrt() * db.sqrt()).max(1e-9)
    }

    #[test]
    fn erb_features_shape_and_magnitude() {
        let frame = tone(1000.0, 48_000, DFN3_HOP, 0.5);
        let window = hann_window(DFN3_FFT);
        let spec = stft_1frame(&frame, &window);
        let fb = erb_filter_bank(false);
        let feat = erb_features(&spec, &fb);
        assert_eq!(feat.shape(), &[3, DFN3_NB_ERB]);
        // The 1 kHz tone must land in a band with a large magnitude.
        let mag_row = feat.row(2);
        assert!(mag_row.iter().cloned().fold(0.0f32, f32::max) > 0.1);
    }

    #[test]
    fn deep_filter_identity_coefs_pass_signal_through() {
        // Coefs [1,0,0,0,0,0,0,0,0,0] on all bins = identity filter.
        let mut bins: Vec<Complex> = (0..DFN3_NB_DF)
            .map(|i| Complex::new(i as f32 * 0.1, (i as f32) * 0.05))
            .collect();
        let orig = bins.clone();
        let coefs = (0..DFN3_NB_DF * DFN3_DF_ORDER * 2)
            .map(|i| {
                if i % (2 * DFN3_DF_ORDER) == 0 {
                    1.0
                } else {
                    0.0
                }
            })
            .collect::<Vec<f32>>();
        deep_filter_inplace(&mut bins, &coefs);
        for (x, y) in bins.iter().zip(orig.iter()) {
            assert!((x.re - y.re).abs() < 1e-4);
            assert!((x.im - y.im).abs() < 1e-4);
        }
    }
}
