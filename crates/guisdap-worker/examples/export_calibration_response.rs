//! Export the exact normalized 10-us spectral response, including partial gates.
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::fs;
fn v<'a>(m: &'a matfile::MatFile, n: &str) -> Result<&'a [f64]> {
    match m.find_by_name(n).context(n.to_string())?.data() {
        matfile::NumericData::Double { real, .. } => Ok(real),
        _ => anyhow::bail!("not double"),
    }
}
fn main() -> Result<()> {
    let m = matfile::MatFile::parse(fs::File::open(
        "legacy/guisdap9/exps/beata/beataT_2init.mat",
    )?)?;
    let freq: Vec<_> = v(&m, "p_om")?
        .iter()
        .map(|w| w * v(&m, "p_om0").unwrap()[0] / (2. * std::f64::consts::PI))
        .collect();
    let nf = freq.len();
    let ra = v(&m, "lpg_ra")?;
    let nt = v(&m, "lpg_nt")?;
    let nd = v(&m, "lpg_ND")?;
    let lag = v(&m, "lpg_lag")?;
    let h = v(&m, "lpg_h")?;
    let dt = v(&m, "lpg_dt")?;
    let wr = v(&m, "lpg_wom")?;
    let ng = ra.len();
    let scale = v(&m, "p_dtau")?[0] * 1e-6 * 299792458. / 2000.;
    let mut kernels = vec![vec![]; 431];
    for i in 0..ng {
        if lag[i] != 25. || !(480. ..25531.).contains(&ra[i]) {
            continue;
        }
        for j in 0..nt[i] as usize {
            let r = (h[i] + j as f64 * dt[i]) * scale;
            let gate = ((r - guisdap_core::ion_short_range_km(0))
                / (guisdap_core::ion_short_range_km(1) - guisdap_core::ion_short_range_km(0)))
            .round() as usize;
            ensure!(
                gate < 431 && (r - guisdap_core::ion_short_range_km(gate)).abs() < 1e-6,
                "range mismatch"
            );
            ensure!(
                (nd[i] - 64. * ((gate / 2 + 1).min(31)) as f64).abs() < 1e-6,
                "normalization mismatch"
            );
            ensure!(kernels[gate].is_empty(), "duplicate short-lag response");
            kernels[gate] = (0..nf)
                .map(|f| {
                    let df = (freq[(f + 1).min(nf - 1)] - freq[f.saturating_sub(1)]) / 2.;
                    wr[i + ng * f] * df / (nd[i] * 156.249999713498)
                })
                .collect();
        }
    }
    ensure!(kernels.iter().all(|k| k.len() == nf), "missing response");
    let bytes = serde_json::to_vec(
        &json!({"frequency_hz":freq,"kernel_real":kernels,"experiment_sha256":"27180ca657512d2bca042824431f688e8b3fa66980bd0916c77aab9c9391f4ec","normalization":"raw_ne = 2 Ne integral(S kernel), same peak 156.249999713498 and ND as ion_short_raw_ne"}),
    )?;
    fs::write("/srv/guisdap/data/beata-short-lag-response.json", bytes)?;
    Ok(())
}
