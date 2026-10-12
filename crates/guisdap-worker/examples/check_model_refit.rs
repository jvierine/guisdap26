//! Validate model-prescribed refits. Changed physics can increase residuals.
use anyhow::{ensure, Result};
use serde_json::Value;
use std::{collections::BTreeMap, fs};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(args.len() == 3, "old-index new-index");
    let old: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let new: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    ensure!(new["processing"]["phase"] == "complete", "incomplete refit");
    ensure!(new["processing"]["failed"] == 0, "failed intervals");
    let a = old["profiles"].as_array().unwrap();
    let b = new["profiles"].as_array().unwrap();
    ensure!(a.len() == b.len(), "time grid mismatch");
    let mut count = 0;
    let mut improved = 0;
    let mut low_old = 0;
    let mut low_new = 0;
    let mut old_status = BTreeMap::<String, usize>::new();
    let mut new_status = old_status.clone();
    for (a, b) in a.iter().zip(b) {
        ensure!(
            a["start_unix"] == b["start_unix"] && a["end_unix"] == b["end_unix"],
            "time mismatch"
        );
        let ga = a["gates"].as_array().unwrap();
        let gb = b["gates"].as_array().unwrap();
        ensure!(ga.len() == gb.len(), "gate count mismatch");
        for (a, b) in ga.iter().zip(gb) {
            ensure!(
                a["lower_km"] == b["lower_km"] && a["upper_km"] == b["upper_km"],
                "altitude mismatch"
            );
            let Some(oldcost) = a["result"]["chi_square"].as_f64() else {
                continue;
            };
            let newcost = b["result"]["chi_square"]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("lost fit"))?;
            ensure!(newcost.is_finite(), "nonfinite objective");
            ensure!(
                b["settings"]["fit_collision_rate"] == false,
                "collision frequency was fitted"
            );
            let nu = b["settings"]["fixed_collision_s"].as_f64().unwrap();
            ensure!(nu.is_finite() && nu > 0., "missing modeled collision rate");
            ensure!(
                b["result"]["plasma"]["ion_collision_s"].as_f64() == Some(nu),
                "fit changed prescribed collision rate"
            );
            ensure!(
                b["result"]["standard_error"][4].is_null(),
                "collision parameter has fitted uncertainty"
            );
            ensure!(
                b["prescribed_atmosphere"]["collision_s"].as_f64() == Some(nu),
                "missing atmospheric provenance"
            );
            let fraction = b["prescribed_atmosphere"]["molecular_fraction"]
                .as_f64()
                .unwrap();
            ensure!(
                (b["oxygen_fraction"].as_f64().unwrap() + fraction - 1.).abs() < 1e-12,
                "composition mismatch"
            );
            let old_dof = a["result"]["degrees_of_freedom"].as_u64().unwrap();
            let delta = u64::from(a["settings"]["fit_collision_rate"] == true);
            ensure!(
                b["result"]["degrees_of_freedom"].as_u64() == Some(old_dof + delta),
                "measurement count changed"
            );
            count += 1;
            if newcost < oldcost - 1e-3 * (1. + oldcost) {
                improved += 1;
            }
            *old_status
                .entry(a["status"].as_str().unwrap().into())
                .or_default() += 1;
            *new_status
                .entry(b["status"].as_str().unwrap().into())
                .or_default() += 1;
            let h = a["altitude_km"].as_f64().unwrap();
            if (100. ..150.).contains(&h) {
                if a["result"]["plasma"]["ion_temperature_k"].as_f64().unwrap() < 250. {
                    low_old += 1;
                }
                if b["result"]["plasma"]["ion_temperature_k"].as_f64().unwrap() < 250. {
                    low_new += 1;
                }
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"checked_fits":count,"improved_objective":improved,"low_Ti_100_150km_before":low_old,"low_Ti_100_150km_after":low_new,"old_status":old_status,"new_status":new_status})
    );
    Ok(())
}
