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
