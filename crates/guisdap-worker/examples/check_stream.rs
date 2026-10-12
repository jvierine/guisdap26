use anyhow::{ensure, Result};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
};
fn main() -> Result<()> {
    let root = PathBuf::from(std::env::args().nth(1).expect("internal output directory"));
    let public = PathBuf::from(std::env::args().nth(2).expect("public output directory"));
    let h = hdf5::File::open(root.join("index.h5"))?;
    let index: Value = serde_json::from_slice(&h.dataset("manifest_json")?.read_raw::<u8>()?)?;
    let web: Value = serde_json::from_slice(&std::fs::read(public.join("index.json"))?)?;
    ensure!(
        index["profiles"] == web["profiles"],
        "internal/public index mismatch"
    );
    let cadence = index["config"]["integration_s"].as_f64().unwrap();
    let expected_sources =
        std::fs::read_dir(index["config"]["quicklook_frames"].as_str().unwrap())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "h5"))
            .count();
    let mut seen = BTreeSet::new();
    let mut statuses = BTreeMap::new();
    let mut negative = 0;
    let mut previous = None;
    for p in index["profiles"].as_array().unwrap() {
        let start = p["start_unix"].as_f64().unwrap();
        let end = p["end_unix"].as_f64().unwrap();
        ensure!((end - start - cadence).abs() < 1e-6, "window duration");
        if let Some(previous) = previous {
            ensure!(start == previous, "noncontiguous nominal windows");
        }
        previous = Some(end);
        ensure!(
            p["processing_state"] != "pending" && p["processing_state"] != "failed",
            "incomplete processing"
        );
        let h = hdf5::File::open(root.join("profiles").join(format!("{start:.0}.h5")))?;
        let detail: Value = serde_json::from_slice(&h.dataset("profile_json")?.read_raw::<u8>()?)?;
        let source = detail["sources"].as_array().unwrap();
        ensure!(
            source.len() == p["source_count"].as_u64().unwrap() as usize,
            "source count"
        );
        for m in source {
            ensure!(
                seen.insert(m["source_sha256"].as_str().unwrap().to_string()),
                "duplicate source"
            );
            let middle = (m["start_unix"].as_f64().unwrap() + m["end_unix"].as_f64().unwrap()) / 2.;
            ensure!(
                middle >= start && middle < end,
                "source assigned to wrong interval"
            );
        }
        let export: Value = serde_json::from_slice(&std::fs::read(
            public.join(p["detail_url"].as_str().unwrap()),
        )?)?;
        ensure!(
            export.get("sources").is_none(),
            "private paths in public detail"
        );
        for (j, g) in detail["gates"].as_array().unwrap().iter().enumerate() {
            *statuses
                .entry(g["status"].as_str().unwrap().to_string())
                .or_insert(0usize) += 1;
            ensure!(
                g["result"] == export["gates"][j]["result"],
                "fit export mismatch"
            );
            if let Some(curve) = g["result"]["model_curve"].as_array() {
                let ds = h.dataset(&format!("profile/gate_{j:03}/model_curve"))?;
                ensure!(ds.shape() == [curve.len(), 5], "model curve shape mismatch");
            }
        }
        let raw = h.dataset("profile/raw_ne_and_sigma")?.read_raw::<f64>()?;
        ensure!(
            raw.len() == detail["raw_ne"].as_array().unwrap().len() * 2,
            "raw density dimensions"
        );
        negative += raw.as_chunks::<2>().0.iter().filter(|p| p[0] < 0.).count();
    }
    ensure!(
        seen.len() == expected_sources,
        "source coverage incomplete: {} of {}",
        seen.len(),
        expected_sources
    );
    // Sidecar for processing clients; also refreshes old run exports after validation.
    let mut tmp = tempfile::NamedTempFile::new_in(&public)?;
    tmp.write_all(&serde_json::to_vec(&web["processing"])?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o644))?;
    }
    tmp.persist(public.join("status.json"))
        .map_err(|e| e.error)?;
    println!("PASS {} intervals at {cadence}s, {} unique source records, {negative} signed negative raw-Ne estimates; statuses {statuses:?}",index["profiles"].as_array().unwrap().len(),seen.len());
    Ok(())
}
