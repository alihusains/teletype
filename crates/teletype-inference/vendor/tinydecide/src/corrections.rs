//! Corrections: turn stored examples into the [`Protos`] argument of
//! [`TinyDecide::answer_with`](crate::TinyDecide::answer_with). Same recipe as the playground and
//! `corrections.js`: one prototype per option (the mean of its examples), a centre (the mean of every
//! vector seen for this question, or of the examples when none is given), and a trust weight `lam`
//! chosen by leave-one-out over the examples themselves.
//!
//! An example is what the model returned for a message, stored under the option a person said was
//! right (for noul questions, list 1 is "true" and list 0 is "false").

use serde::{Deserialize, Serialize};

use crate::engine::{bucket, Answer, Protos, QType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    /// `answer.qvec`
    pub v: Vec<f64>,
    /// `answer.z0`
    pub z: Vec<f64>,
}

impl Example {
    pub fn from_answer(a: &Answer) -> Self {
        Example { v: a.qvec.iter().map(|&x| x as f64).collect(), z: a.z0.clone() }
    }
}

/// The trust weights tried by [`lambda_for`].
pub const LAMBDAS: [f64; 5] = [0.0, 0.25, 0.5, 1.0, 2.0];

pub fn mean_vec(list: &[&Example], dh: usize) -> Vec<f64> {
    let mut m = vec![0f64; dh];
    let n = list.len() as f64;
    for e in list {
        for (mi, vi) in m.iter_mut().zip(&e.v) {
            *mi += vi / n;
        }
    }
    m
}

fn cos_c(a: &[f64], b: &[f64], c: &[f64]) -> f64 {
    let (mut d, mut na, mut nb) = (0f64, 0f64, 0f64);
    for i in 0..a.len() {
        let x = a[i] - c[i];
        let y = b[i] - c[i];
        d += x * y;
        na += x * x;
        nb += y * y;
    }
    let n = (na * nb).sqrt();
    d / if n == 0.0 || n.is_nan() { 1e-12 } else { n }
}

fn term_for(v: &[f64], lists: &[Vec<&Example>], c: &[f64], beta: &[f64]) -> Vec<f64> {
    lists
        .iter()
        .map(|l| if l.is_empty() { 0.0 } else { beta[bucket(l.len() as i32)] * cos_c(v, &mean_vec(l, v.len()), c) })
        .collect()
}

/// How much to trust a question's corrections: leave-one-out log-likelihood over the examples.
pub fn lambda_for(kind: QType, lists: &[Vec<Example>], c: &[f64], beta: &[f64]) -> f64 {
    let all: Vec<(usize, usize, &Example)> =
        lists.iter().enumerate().flat_map(|(k, l)| l.iter().enumerate().map(move |(j, e)| (k, j, e))).collect();
    if all.len() < 2 {
        return 0.25;
    }
    let (mut best, mut best_ll) = (0.0, f64::NEG_INFINITY);
    for lam in LAMBDAS {
        let mut ll = 0.0;
        for &(k, j, e) in &all {
            let rest: Vec<Vec<&Example>> = lists
                .iter()
                .enumerate()
                .map(|(kk, l)| l.iter().enumerate().filter(|&(jj, _)| kk != k || jj != j).map(|(_, x)| x).collect())
                .collect();
            let t = term_for(&e.v, &rest, c, beta);
            let z: Vec<f64> = if kind == QType::Noul {
                vec![lam * t[0], e.z.get(1).copied().unwrap_or(0.0) + lam * t.get(1).copied().unwrap_or(0.0)]
            } else {
                e.z.iter().enumerate().map(|(i, x)| x + lam * t.get(i).copied().unwrap_or(f64::NAN)).collect()
            };
            let m = z.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let lse = m + z.iter().map(|x| (x - m).exp()).sum::<f64>().ln();
            ll += z.get(k).copied().unwrap_or(f64::NAN) - lse;
        }
        if ll > best_ll + 1e-9 {
            best = lam;
            best_ll = ll;
        }
    }
    best
}

/// `lists`: one list of examples per option (2 for noul). `beta`: [`TinyDecide::beta`](crate::TinyDecide::beta).
/// `center`: optional running mean of `qvec` over every message asked with this question.
/// Returns `None` when there are no examples (span questions take none).
pub fn make_protos(kind: QType, lists: &[Vec<Example>], beta: &[f64], center: Option<&[f64]>) -> Option<Protos> {
    if kind == QType::Span {
        return None;
    }
    let dh = lists.iter().find(|l| !l.is_empty())?[0].v.len();
    let c: Vec<f64> = match center {
        Some(c) => c.to_vec(),
        None => mean_vec(&lists.iter().flatten().collect::<Vec<_>>(), dh),
    };
    let mut vec = vec![0f32; lists.len() * dh];
    for (k, l) in lists.iter().enumerate() {
        if !l.is_empty() {
            let m = mean_vec(&l.iter().collect::<Vec<_>>(), dh);
            for (i, x) in m.iter().enumerate() {
                vec[k * dh + i] = *x as f32;
            }
        }
    }
    Some(Protos {
        vec,
        cnt: lists.iter().map(|l| l.len() as i32).collect(),
        center: Some(c.iter().map(|&x| x as f32).collect()),
        lam: Some(lambda_for(kind, lists, &c, beta)),
    })
}
