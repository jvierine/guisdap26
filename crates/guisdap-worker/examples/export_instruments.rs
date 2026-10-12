//! Export native-record instrument diagnostics without rerunning fits.
#[path = "../src/mat4.rs"]
#[allow(dead_code)]
mod mat4;
use anyhow::{ensure, Result};
use guisdap_core::FrameMeta;
use rayon::prelude::*;
use serde_json::json;
use std::{fs, io::Read, path::PathBuf};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 4,
        "usage: export_instruments INPUT_FRAMES OUTPUT.h5 OUTPUT.json"
    );
    let mut metas = Vec::new();
    for entry in fs::read_dir(&args[1])? {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "h5") {
            let h = hdf5::File::open(path)?;
            metas.push(serde_json::from_slice::<FrameMeta>(
                &h.dataset("metadata_json")?.read_raw::<u8>()?,
            )?);
        }
    }
    metas.sort_by(|a, b| a.start_unix.total_cmp(&b.start_unix));
    let records: Result<Vec<_>> = rayon::ThreadPoolBuilder::new().num_threads(8).build()?.install(|| metas.par_iter().map(|m| {
        let path = PathBuf::from(&m.source);
        let file = fs::File::open(&path)?;
        let mut reader: Box<dyn Read> = if path.extension().is_some_and(|x| x == "bz2") { Box::new(bzip2::read::BzDecoder::new(file)) } else { Box::new(file) };
        let mut bytes = Vec::new(); reader.read_to_end(&mut bytes)?;
        let arrays = mat4::parse(&bytes)?;
        let par = &arrays["d_parbl"].real;
        let data = &arrays["d_data"].real;
        ensure!(data.len() == 196493 && par.len() > 20, "unsupported BEATA layout");
        let background = data[..16].iter().sum::<f64>() / 512.;
        let injected = data[16..32].iter().sum::<f64>() / 512.;
        let tsys = (background >= 0. && injected > background && par[20] > 0.).then(|| background * par[20] / (injected - background));
        Ok(json!({"start_unix":m.start_unix,"end_unix":m.end_unix,"system_temperature_k":tsys,
            "transmitter_w":m.transmitter_w,"azimuth_deg":m.azimuth_deg,"elevation_deg":m.elevation_deg}))
    }).collect());
    let records = records?;
    let doc = json!({"schema":"beata22-instruments-v1","temperature_method":"Ion channel: Tsys = background * Tcal / (calibration_on - background); d_data[0..16] and [16..32], common 1/512 normalization; Tcal=d_parbl[20], zero-based. Receiver plus sky noise; absolute calibration uncertainty not estimated.","records":records});
    let bytes = serde_json::to_vec(&doc)?;
    {
        let h = hdf5::File::create(&args[2])?;
        h.new_dataset_builder()
            .with_data(bytes.as_slice())
            .create("instrument_json")?;
    }
    let out = PathBuf::from(&args[3]);
    let tmp = out.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, out)?;
    println!("Exported {} instrument records", records.len());
    Ok(())
}
