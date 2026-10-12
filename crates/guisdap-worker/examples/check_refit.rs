//! Reject lost measurements or worsened objectives before publishing a refit.
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
            ensure!(
                newcost.is_finite() && newcost <= oldcost + 1e-6 * (1. + oldcost),
                "worse objective at {} km: {oldcost} -> {newcost}",
                a["altitude_km"]
            );
            ensure!(
                a["result"]["degrees_of_freedom"] == b["result"]["degrees_of_freedom"],
                "changed measurement/model count"
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
