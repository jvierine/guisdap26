use anyhow::{ensure, Result};
use serde_json::Value;
use std::collections::BTreeMap;
fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("HDF5 product path");
    let h = hdf5::File::open(path)?;
    let data: Value = serde_json::from_slice(
        &h.dataset("metadata_and_diagnostics_json")?
            .read_raw::<u8>()?,
    )?;
    let profiles = data["profiles"].as_array().unwrap();
    if let Some(previous) = std::env::args().nth(2) {
        let old: Value = serde_json::from_slice(&std::fs::read(previous)?)?;
        ensure!(
            old["profiles"].as_array().unwrap().len() == profiles.len(),
            "profile count changed"
        );
        for (p, oldp) in profiles.iter().zip(old["profiles"].as_array().unwrap()) {
            for (g, oldg) in p["gates"]
                .as_array()
                .unwrap()
                .iter()
                .zip(oldp["gates"].as_array().unwrap())
            {
                let mut current = g["result"].clone();
                if let Some(r) = current.as_object_mut() {
                    r.remove("model_curve");
                }
                ensure!(current == oldg["result"], "existing fit result changed");
            }
        }
        println!("PASS existing fit results unchanged");
    }
    let mut counts = BTreeMap::new();
    for (i, p) in profiles.iter().enumerate() {
        let gates = p["gates"].as_array().unwrap();
        let values = h
            .dataset(&format!("profile_{i:03}/parameters_ne_ti_te_v_nu"))?
            .read_raw::<f64>()?;
        ensure!(values.len() == gates.len() * 5, "wrong output shape");
        for (j, g) in gates.iter().enumerate() {
            let status = g["status"].as_str().unwrap();
            *counts.entry(status.to_string()).or_insert(0) += 1;
            let n = g["observed"].as_array().unwrap().len();
            ensure!(g["lag_us"].as_array().unwrap().len() == n, "lag count");
            if let Some(r) = g["result"].as_object() {
                ensure!(
                    r["predicted"].as_array().unwrap().len() == n,
                    "prediction count"
                );
                let curve = r["model_curve"].as_array().unwrap();
                let ds = h.dataset(&format!("profile_{i:03}/gate_{j:03}/model_curve"))?;
                ensure!(ds.shape() == [curve.len(), 5], "model curve shape");
                let stored = ds.read_raw::<f64>()?;
                for (row, point) in stored.as_chunks::<5>().0.iter().zip(curve) {
                    let expected = [
                        point["lag_s"].as_f64().unwrap(),
                        point["value"][0].as_f64().unwrap(),
                        point["value"][1].as_f64().unwrap(),
                        point["standard_error"][0].as_f64().unwrap_or(f64::NAN),
                        point["standard_error"][1].as_f64().unwrap_or(f64::NAN),
                    ];
                    for (&a, b) in row.iter().zip(expected) {
                        ensure!(
                            (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-12 * b.abs().max(1.),
                            "model curve HDF5/JSON disagreement"
                        );
                    }
                    ensure!(
                        row[3..]
                            .iter()
                            .all(|e| e.is_nan() || (e.is_finite() && *e >= 0.)),
                        "invalid model error"
                    );
                }
                let ne = r["plasma"]["electron_density_m3"].as_f64().unwrap();
                ensure!((values[j * 5] - ne).abs() < 1., "HDF5/JSON disagreement");
            }
            if i == 5 {
                println!("h={:5.1} {:20} groups={:3} width={:5.1} Ne={:9.3e} Ti={:6.0} Te={:6.0} v={:7.1} nu={:8.1} score={:6.2}",g["altitude_km"].as_f64().unwrap(),status,n,g["response_width_max_km"].as_f64().unwrap(),values[j*5],values[j*5+1],values[j*5+2],values[j*5+3],values[j*5+4],g["result"]["reduced_chi_square"].as_f64().unwrap_or(f64::NAN));
            }
        }
    }
    println!("PASS {} profiles, statuses {counts:?}", profiles.len());
    Ok(())
}
