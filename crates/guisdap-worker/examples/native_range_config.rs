//! Derive native first-lag range cells from the experiment, without guessed spacing.
use anyhow::{ensure, Context, Result};
use guisdap_core::{altitude_km, FrameMeta};
use serde_json::{json, Value};
use std::{fs, path::Path};
fn values<'a>(m: &'a matfile::MatFile, n: &str) -> Result<&'a [f64]> {
    match m.find_by_name(n).context(n.to_string())?.data() {
        matfile::NumericData::Double { real, .. } => Ok(real),
        _ => anyhow::bail!("not double: {n}"),
    }
}
fn main() -> Result<()> {
    let mut cfg: Value = serde_json::from_slice(&fs::read("config/ion-fit-full-15s.json")?)?;
    let model = matfile::MatFile::parse(fs::File::open(
        cfg["experiment_model"].as_str().context("model")?,
    )?)?;
    let frames = Path::new(cfg["quicklook_frames"].as_str().context("frames")?);
    let file = fs::read_dir(frames)?.next().context("empty inputs")??;
    let meta: FrameMeta = serde_json::from_slice(
        &hdf5::File::open(file.path())?
            .dataset("metadata_json")?
            .read_raw::<u8>()?,
    )?;
    let (lag, h, dt, nt, ra, bcs) = (
        values(&model, "lpg_lag")?,
        values(&model, "lpg_h")?,
        values(&model, "lpg_dt")?,
        values(&model, "lpg_nt")?,
        values(&model, "lpg_ra")?,
        values(&model, "lpg_bcs")?,
    );
    let scale = values(&model, "p_dtau")?[0] * 1e-6 * 299792458. / 2000.;
    let mut range = vec![];
    for i in 0..lag.len() {
        if bcs[i] == 115. && lag[i] == 25. && (480. ..25531.).contains(&ra[i]) {
            for j in 0..nt[i] as usize {
                range.push((h[i] + j as f64 * dt[i]) * scale);
            }
        }
    }
    range.sort_by(f64::total_cmp);
    range.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
    ensure!(range.len() == 431, "unexpected native first-lag grid");
    let mut steps: Vec<_> = (0..lag.len())
        .filter(|&i| bcs[i] == 115. && (480. ..25531.).contains(&ra[i]) && nt[i] > 1.)
        .map(|i| dt[i] * scale)
        .collect();
    steps.sort_by(f64::total_cmp);
    steps.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    println!("Native group slant spacings: {steps:?}");
    let step = range[1] - range[0];
    ensure!(
        range
            .windows(2)
            .all(|x| ((x[1] - x[0]) - step).abs() < 1e-8),
        "nonuniform native grid"
    );
    let mut slant_edges = vec![range[0] - step / 2.];
    let mut edges = vec![altitude_km(range[0] - step / 2., meta.elevation_deg)];
    for r in &range {
        let edge = altitude_km(r + step / 2., meta.elevation_deg);
        edges.push(edge);
        slant_edges.push(r + step / 2.);
        if edge >= 250. {
            break;
        }
    }
    let native_cells = edges.len() - 1;
    let boundary = *edges.last().unwrap();
    for e in cfg["altitude_edges_km"].as_array().context("edges")? {
        let e = e.as_f64().context("edge")?;
        if e > boundary {
            edges.push(e);
        }
    }
    cfg["altitude_edges_km"] = json!(edges);
    cfg["native_range_below_km"] = json!(boundary);
    cfg["native_slant_edges_km"] = json!(slant_edges);
    cfg["threads"] = json!(128);
    for seconds in [15, 60] {
        let name = format!("2022-11-02-ionfit-native-{seconds}s-v2");
        cfg["integration_s"] = json!(seconds);
        cfg["output"] = json!(format!("/home/j/data/guisdap/{name}"));
        cfg["export"] = json!(format!("/srv/guisdap/data/{name}"));
        cfg["live_export"] = Value::Null;
        fs::write(
            format!("config/ion-fit-native-{seconds}s.json"),
            serde_json::to_vec_pretty(&cfg)?,
        )?;
    }
    println!("{native_cells} native cells below {boundary:.6} km; slant step {step:.9} km; elevation {} deg; total {} cells",meta.elevation_deg,edges.len()-1);
    Ok(())
}
