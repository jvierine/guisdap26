use anyhow::{ensure, Result};
use serde_json::Value;
use std::{collections::BTreeSet, fs};
fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .expect("internal profiles directory");
    let mut profiles = 0;
    let mut gates = 0;
    for e in fs::read_dir(root)? {
        let path = e?.path();
        if path.extension().is_none_or(|e| e != "h5") {
            continue;
        }
        let h = hdf5::File::open(path)?;
        let p: Value = serde_json::from_slice(&h.dataset("profile_json")?.read_raw::<u8>()?)?;
        ensure!(
            p["processing_state"] != "failed",
            "failed interval {:?}",
            p["notes"]
        );
        if p["processing_state"] != "complete" {
            continue;
        }
        profiles += 1;
        let mut addresses = BTreeSet::new();
        for g in p["gates"].as_array().unwrap() {
            if g["altitude_km"].as_f64().unwrap() > 250. {
                continue;
            }
            ensure!(
                g["upper_km"].as_f64().unwrap() - g["lower_km"].as_f64().unwrap() < 1.5,
                "range averaging remains"
            );
            gates += 1;
            for group in g["addresses"].as_array().unwrap() {
                let a = group.as_array().unwrap();
                ensure!(a.len() == 1, "native group contains {} samples", a.len());
                ensure!(
                    addresses.insert(a[0].as_u64().unwrap()),
                    "native sample duplicated across range cells"
                );
            }
        }
    }
    ensure!(profiles > 0, "no completed profiles to validate");
    println!("PASS {profiles} completed profiles, {gates} native cells: one address per response group, no cross-cell duplicates");
    Ok(())
}
