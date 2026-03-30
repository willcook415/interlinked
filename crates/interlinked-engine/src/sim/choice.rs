use super::graph::BuiltPath;

pub(crate) fn logit_shares(paths: &[BuiltPath], theta: f64) -> Vec<f64> {
    if paths.is_empty() {
        return vec![];
    }

    if theta <= 0.0 {
        return vec![1.0 / paths.len() as f64; paths.len()];
    }

    let mut u = Vec::with_capacity(paths.len());
    let mut umax = f64::NEG_INFINITY;
    for p in paths {
        let v = -theta * p.stats.gc_s;
        if v > umax {
            umax = v;
        }
        u.push(v);
    }

    let mut exps = Vec::with_capacity(paths.len());
    let mut sum = 0.0;
    for v in u {
        let e = (v - umax).exp();
        sum += e;
        exps.push(e);
    }

    if sum <= 0.0 {
        return vec![1.0 / paths.len() as f64; paths.len()];
    }

    exps.into_iter().map(|e| e / sum).collect()
}
