use anyhow::{ensure, Result};
use serde_json::Value;
use std::{collections::BTreeSet, path::PathBuf};
fn main() -> Result<()> {
    let input = PathBuf::from(std::env::args().nth(1).expect("average HDF5 directory"));
    let public = PathBuf::from(std::env::args().nth(2).expect("export directory"));
    let index: Value = serde_json::from_slice(&std::fs::read(public.join("index.json"))?)?;
    let frames = index["frames"].as_array().unwrap();
    let mut sources = BTreeSet::new();
    let mut partial = 0;
    let mut count = 0;
    for frame in frames {
        let id = frame["id"].as_str().unwrap();
        let h = hdf5::File::open(input.join("frames").join(format!("{id}.h5")))?;
        let original: Value =
            serde_json::from_slice(&h.dataset("source_records_json")?.read_raw::<u8>()?)?;
        let records = original.as_array().unwrap();
        ensure!(
            records.len() == frame["source_count"].as_u64().unwrap() as usize,
            "source count mismatch"
        );
        let start = frame["start_unix"].as_f64().unwrap();
        let end = frame["end_unix"].as_f64().unwrap();
        if records.len() as f64 * 5. < end - start {
            partial += 1;
        }
        for source in records {
            ensure!(
                sources.insert(source["source_sha256"].as_str().unwrap().to_string()),
                "duplicate source"
            );
            let t = (source["start_unix"].as_f64().unwrap() + source["end_unix"].as_f64().unwrap())
                / 2.;
            ensure!(t >= start && t < end, "wrong averaging window");
        }
        let bytes = std::fs::read(public.join("frames").join(id).join("native-long.f32"))?;
        ensure!(
            bytes.len() == 2 * 800 * 90 * 2 * 4,
            "native lag export dimensions"
        );
        if count == 0 || count == frames.len() / 2 || count + 1 == frames.len() {
            let native = h.dataset("native_long_acf")?.read_raw::<f32>()?;
            let fft = guisdap_core::native_spectra(&native, true)?;
            let stored = h.dataset("decoded_spectrum")?.read_raw::<f32>()?;
            let max = stored.iter().map(|x| x.abs()).fold(1f32, f32::max);
            let delta = fft
                .iter()
                .zip(stored)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            ensure!(
                delta < max * 2e-5,
                "FFT of averaged lags differs from average FFT: {delta} / {max}"
            );
        }
        count += 1;
    }
    ensure!(sources.len() == 2869, "incomplete whole-study coverage");
    let root = public.join(index["overview_root"].as_str().unwrap());
    for (name, rows) in [
        ("broad-raw.f32", 800),
        ("broad-excess.f32", 800),
        ("ion-short-raw-ne.f32", 431),
        ("ion-short-raw-ne-sigma.f32", 431),
    ] {
        ensure!(
            std::fs::metadata(root.join(name))?.len() == (rows * frames.len() * 4) as u64,
            "overview dimensions {name}"
        );
    }
    println!("PASS {count} plasma intervals; {} unique sources; {partial} partial-record windows; FFT linearity, complete native lags and overview dimensions verified",sources.len());
    Ok(())
}
