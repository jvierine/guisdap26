//! Read-only comparison of fitted and short-lag density by altitude.
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, fs};
fn q(x: &mut [f64], f: f64) -> f64 {
    if x.is_empty() {
        return f64::NAN;
    }
    x.sort_by(f64::total_cmp);
    x[((x.len() - 1) as f64 * f).round() as usize]
}
fn main() -> Result<()> {
    for path in std::env::args().skip(1) {
        let data: Value = serde_json::from_slice(&fs::read(&path)?)?;
        let profiles = data["profiles"].as_array().context("profiles missing")?;
        println!("{path}");
        for gi in 3..15 {
            let mut ratios = vec![];
            let mut temp = vec![];
            let mut collision = vec![];
            let mut errors = vec![];
            let mut scores = vec![];
            let mut statuses = BTreeMap::<String, usize>::new();
            let mut weak_accepted = 0;
            let mut n = 0;
            for p in profiles {
                let g = &p["gates"][gi];
                let r = &g["result"];
                let Some(ne) = r["plasma"]["electron_density_m3"].as_f64() else {
                    continue;
                };
                n += 1;
                *statuses
                    .entry(g["status"].as_str().unwrap_or("missing").to_string())
                    .or_default() += 1;
                let score = r["reduced_chi_square"].as_f64().unwrap_or(f64::NAN);
                scores.push(score);
                if score <= 10. && g["status"] != "converged" {
                    weak_accepted += 1;
                }
                if let Some(raw) = p["raw_ne"][gi][0].as_f64().filter(|x| *x > 0.) {
                    ratios.push(ne / raw);
                }
                if let (Some(te), Some(ti)) = (
                    r["plasma"]["electron_temperature_k"].as_f64(),
                    r["plasma"]["ion_temperature_k"].as_f64(),
                ) {
                    temp.push(te / ti);
                }
                if let Some(c) = r["plasma"]["ion_collision_s"].as_f64() {
                    collision.push(c);
                }
                if let Some(e) = r["standard_error"][0].as_f64() {
                    errors.push(e / ne);
                }
            }
            println!("h={} n={n} Ne/raw p10,50,90={:.2},{:.2},{:.2} Te/Ti p50,90={:.2},{:.2} nu p50,90={:.1},{:.1} relerr p50,90={:.2},{:.2} score50={:.2} nonconverged_under10={weak_accepted} {:?}", profiles[0]["gates"][gi]["altitude_km"],q(&mut ratios,0.1),q(&mut ratios,0.5),q(&mut ratios,0.9),q(&mut temp,0.5),q(&mut temp,0.9),q(&mut collision,0.5),q(&mut collision,0.9),q(&mut errors,0.5),q(&mut errors,0.9),q(&mut scores,0.5),statuses);
        }
    }
    Ok(())
}
