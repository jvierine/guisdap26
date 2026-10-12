mod ion;
mod ion_index;
mod mat4;
mod plasma_average;
mod study;
use anyhow::{ensure, Context, Result};
use axum::response::IntoResponse;
use chrono::{NaiveDate, TimeZone, Utc};
use clap::{Parser, Subcommand};
use guisdap_core::{self as core, FrameMeta};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const SCHEMA: &str = "beata22-quicklook-v5";
#[derive(Parser)]
#[command(version, about = "Rust BEATA 2.2 plasma-line quick-look processor")]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    RunStudy {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        monitor_only: bool,
    },
    AveragePlasma {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        integration_s: f64,
        #[arg(long, default_value_t = 128)]
        threads: usize,
    },
    IndexIon {
        #[arg(long)]
        config: PathBuf,
    },
    FitIon {
        #[arg(long)]
        config: PathBuf,
    },
    Analyze {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value_t = 0)]
        node_index: usize,
        #[arg(long, default_value_t = 1)]
        node_count: usize,
    },
    Inspect {
        file: PathBuf,
    },
    Export {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Serve {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8088")]
        bind: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    roots: Vec<PathBuf>,
    output: PathBuf,
    start: String,
    end: String,
    threads: usize,
}
fn load(path: &Path) -> Result<(Vec<u8>, BTreeMap<String, mat4::Array>)> {
    let f = fs::File::open(path)?;
    let mut bytes = vec![];
    let mut r: Box<dyn Read> = if path.extension().is_some_and(|s| s == "bz2") {
        Box::new(bzip2::read::BzDecoder::new(f))
    } else {
        Box::new(f)
    };
    r.by_ref()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "record exceeds 64 MiB reader limit"
    );
    let arrays = mat4::parse(&bytes).with_context(|| format!("reading {}", path.display()))?;
    Ok((bytes, arrays))
}
fn timestamp(p: &[f64]) -> Result<f64> {
    ensure!(
        p.len() >= 67 && p.iter().all(|x| x.is_finite()),
        "invalid parameter block"
    );
    let d =
        NaiveDate::from_ymd_opt(p[0] as i32, p[1] as u32, p[2] as u32).context("invalid date")?;
    let d = d
        .and_hms_micro_opt(
            p[3] as u32,
            p[4] as u32,
            p[5].floor() as u32,
            ((p[5] % 1.0) * 1e6).round() as u32,
        )
        .context("invalid UTC time")?;
    Ok(Utc.from_utc_datetime(&d).timestamp_micros() as f64 / 1e6)
}
fn metadata(path: &Path, bytes: &[u8], m: &BTreeMap<String, mat4::Array>) -> Result<FrameMeta> {
    let p = &m.get("d_parbl").context("missing d_parbl")?.real;
    let end = timestamp(p)?;
    let exp = m
        .get("d_ExpInfo")
        .context("missing experiment identity")?
        .text();
    ensure!(
        exp.contains("beata_") && exp.contains("2.2u"),
        "unsupported experiment: {exp}"
    );
    ensure!(
        (p[56] - 2.2).abs() < 1e-6 && p[40] == 4.0,
        "expected BEATA 2.2 UHF metadata"
    );
    ensure!(
        p[6] > 0.0 && p[6] <= 60.0 && (0.0..=90.0).contains(&p[8]),
        "invalid integration/pointing"
    );
    ensure!(
        (p[46] + 2.5).abs() < 1e-6 && (p[47] + 4.9).abs() < 1e-6,
        "unexpected receiver tuning"
    );
    let hash = format!("{:x}", Sha256::digest(bytes));
    let id = format!("{}-{}", (end * 1e6).round() as i64, &hash[..16]);
    Ok(FrameMeta {
        id,
        source: path.display().to_string(),
        source_sha256: hash,
        start_unix: end - p[6],
        end_unix: end,
        elevation_deg: p[8],
        azimuth_deg: p[9],
        centers_hz: [p[46] * 1e6, p[47] * 1e6],
        integration_s: p[6],
        transmitter_w: p[7],
        schema: SCHEMA.into(),
    })
}
fn dataset(
    file: &hdf5::File,
    name: &str,
    shape: &[usize],
    values: &[f32],
    units: &str,
) -> Result<()> {
    let d = file
        .new_dataset::<f32>()
        .shape(shape)
        .deflate(4)
        .create(name)?;
    d.write_raw(values)?;
    d.new_attr_builder().with_data(units).create("units")?;
    Ok(())
}
fn write_frame(dir: &Path, meta: &FrameMeta, p: &core::Products, par: &[f64]) -> Result<()> {
    fs::create_dir_all(dir)?;
    let dest = dir.join(format!("{}.h5", meta.id));
    if dest.exists() {
        let existing = read_meta(&hdf5::File::open(&dest)?)?;
        ensure!(
            existing.schema == SCHEMA && existing.source_sha256 == meta.source_sha256,
            "existing product mismatch"
        );
        return Ok(());
    }
    let tmp = tempfile::Builder::new()
        .suffix(".h5.tmp")
        .tempfile_in(dir)?;
    {
        let f = hdf5::File::create(tmp.path())?;
        f.new_dataset_builder()
            .with_data(serde_json::to_vec(meta)?.as_slice())
            .create("metadata_json")?;
        f.new_dataset_builder().with_data(par).create("d_parbl")?;
        f.new_dataset_builder()
            .with_data(&p.background)
            .create("noise_reference")?;
        f.new_dataset_builder()
            .with_data(&(1u32..=800).collect::<Vec<_>>())
            .create("native_long_lag_samples")?;
        f.new_dataset_builder()
            .with_data(&(0u32..50).collect::<Vec<_>>())
            .create("native_short_lag_samples")?;
        f.new_dataset_builder()
            .with_data(&[0u32])
            .create("fft_unmeasured_lag_samples")?;
        dataset(
            &f,
            "native_long_acf",
            &[2, 800, 90, 2],
            &p.native_long,
            "accumulator / normalization; lags 1..800; real,imag",
        )?;
        dataset(
            &f,
            "native_short_acf",
            &[2, 50, 91, 2],
            &p.native_short,
            "accumulator / normalization; lags 0..49; real,imag",
        )?;
        dataset(
            &f,
            "native_uncoded_acf",
            &[2, 50, 4, 2],
            &p.native_uncoded,
            "undecoded broad-gate accumulator / normalization; lags 0..49; real,imag; diagnostic only",
        )?;
        f.new_dataset_builder()
            .with_data(&(0u32..50).collect::<Vec<_>>())
            .create("native_uncoded_lag_samples")?;
        f.new_dataset_builder()
            .with_data(&[core::NFFT as u32])
            .create("decoded_fft_length")?;
        dataset(
            &f,
            "broad_spectrum",
            &[2, 4, 100],
            &p.broad,
            "accumulator / normalization; FFT convention, no dt scaling",
        )?;
        dataset(
            &f,
            "decoded_spectrum",
            &[2, 90, core::NFFT],
            &p.decoded,
            "signed native FFT contrast; sqrt overlap; missing R0; accumulator seconds",
        )?;
        dataset(
            &f,
            "decoded_lag0",
            &[2, 90],
            &p.decoded_lag0,
            "signed short-block diagnostic / normalization / (31/3); no extrapolation",
        )?;
        let (short_ne, short_se) = core::ion_short_raw_ne(p, par[20], meta.transmitter_w, par[63])?;
        dataset(
            &f,
            "ion_short_acf",
            &[431, 2],
            &p.ion_short_acf,
            "accumulator / normalization; measured 10 us lag; real,imag",
        )?;
        dataset(
            &f,
            "ion_short_slices",
            &[14, 431, 2],
            &p.ion_short_slices,
            "accumulator / normalization; fourteen subintegrations; measured 10 us lag; real,imag",
        )?;
        dataset(&f, "ion_short_raw_ne", &[431], &short_ne, "m^-3; signed 10 us lag raw-Ne equivalent; peak spectral response; Te/Ti=1; no Debye correction")?;
        dataset(&f, "ion_short_raw_ne_sigma", &[431], &short_se, "m^-3; approximate 1-sigma from fourteen subintegrations and calibration; excludes inter-slice covariance and systematic errors")?;
        f.new_dataset_builder()
            .with_data(&[10e-6f64])
            .create("ion_short_lag_s")?;
        f.new_dataset_builder()
            .with_data(&vec![36.23203814516196 * 0.4 * 0.149896229; 431])
            .create("ion_short_response_width_km")?;
        let (ne, se) = core::ion_raw_ne(p, par[20], meta.transmitter_w, par[63])?;
        dataset(
            &f,
            "ion_raw_ne",
            &[432],
            &ne,
            "m^-3; signed zero-lag raw Ne; Te/Ti=1; no Debye correction",
        )?;
        dataset(
            &f,
            "ion_raw_ne_sigma",
            &[432],
            &se,
            "m^-3; approximate 1-sigma local-profile scatter and calibration-sample uncertainty",
        )?;
        f.new_dataset_builder()
            .with_data(&[p.ion_calibration])
            .create("ion_calibration_reference")?;
        dataset(
            &f,
            "ion_power",
            &[432],
            &p.ion_power,
            "accumulator / normalization",
        )?;
        dataset(
            &f,
            "ion_excess",
            &[432],
            &p.ion_excess,
            "(power - noise) / noise",
        )?;
        dataset(
            &f,
            "plasma_power",
            &[2, 5450],
            &p.plasma_power,
            "accumulator / normalization",
        )?;
        dataset(
            &f,
            "plasma_excess",
            &[2, 5450],
            &p.plasma_excess,
            "(power - noise) / noise",
        )?;
        for (name, n, range) in [
            ("ion", 432, core::ion_range_km as fn(usize) -> f64),
            ("ion_short", 431, core::ion_short_range_km),
            ("plasma", 5450, core::plasma_range_km),
            ("decoded", 90, core::decoded_range_km),
        ] {
            let r: Vec<f64> = (0..n).map(range).collect();
            let h: Vec<f64> = r
                .iter()
                .map(|&r| core::altitude_km(r, meta.elevation_deg))
                .collect();
            f.new_dataset_builder()
                .with_data(&r)
                .create(format!("{name}_range_km").as_str())?;
            f.new_dataset_builder()
                .with_data(&h)
                .create(format!("{name}_altitude_km").as_str())?;
        }
        let freq: Vec<f64> = meta
            .centers_hz
            .iter()
            .flat_map(|c| {
                (0..core::NFFT).map(move |i| {
                    c + (i as f64 - (core::NFFT / 2) as f64) / (core::NFFT as f64 * core::DT)
                })
            })
            .collect();
        let ds = f
            .new_dataset::<f64>()
            .shape([2, core::NFFT])
            .create("decoded_frequency_hz")?;
        ds.write_raw(&freq)?;
        f.flush()?;
    }
    tmp.as_file().sync_all()?;
    match tmp.persist_noclobber(&dest) {
        Ok(_) => (),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let old = read_meta(&hdf5::File::open(dest)?)?;
            ensure!(
                old.schema == meta.schema && old.source_sha256 == meta.source_sha256,
                "concurrent output conflict"
            );
        }
        Err(e) => return Err(e.into()),
    };
    Ok(())
}
fn read_meta(f: &hdf5::File) -> Result<FrameMeta> {
    Ok(serde_json::from_slice(
        &f.dataset("metadata_json")?.read_raw::<u8>()?,
    )?)
}
fn analyze(config: PathBuf, node: usize, nodes: usize) -> Result<()> {
    ensure!(
        nodes > 0 && node < nodes,
        "node-index must be < positive node-count"
    );
    let c: Config = serde_json::from_slice(&fs::read(config)?)?;
    ensure!(
        c.threads > 0 && !c.roots.is_empty(),
        "positive threads and at least one root required"
    );
    let start = chrono::DateTime::parse_from_rfc3339(&c.start)?.timestamp_micros() as f64 / 1e6;
    let end = chrono::DateTime::parse_from_rfc3339(&c.end)?.timestamp_micros() as f64 / 1e6;
    ensure!(end > start, "empty interval");
    let mut paths = vec![];
    for root in &c.roots {
        ensure!(root.is_dir(), "input root unavailable: {}", root.display());
        for e in walkdir::WalkDir::new(root).follow_links(false) {
            let e = e?;
            let name = e.file_name().to_string_lossy();
            if e.file_type().is_file()
                && (name.ends_with(".mat.bz2") || name.ends_with(".mat"))
                && name.as_bytes()[0].is_ascii_digit()
            {
                let digest = Sha256::digest(name.as_bytes());
                let shard = u64::from_le_bytes(digest[..8].try_into().unwrap()) % nodes as u64;
                if shard as usize == node {
                    paths.push(e.path().to_path_buf());
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    ensure!(!paths.is_empty(), "no input files assigned to node {node}");
    eprintln!(
        "Node {node}/{nodes}: inspecting {} files across {} roots",
        paths.len(),
        c.roots.len()
    );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(c.threads)
        .build()?;
    let results: Vec<Result<Option<FrameMeta>>> = pool.install(|| {
        paths
            .par_iter()
            .map(|path| {
                let (bytes, m) = load(path)?;
                let par = &m.get("d_parbl").context("missing d_parbl")?.real;
                let t = timestamp(par)?;
                // Select integration midpoints; preserve the actual start/end in every product.
                if t - par[6] / 2.0 < start || t - par[6] / 2.0 >= end {
                    return Ok(None);
                }
                let meta = metadata(path, &bytes, &m)?;
                let data = m.get("d_data").context("missing correlator data")?;
                ensure!(
                    data.cols == 1 && data.rows == 196493,
                    "unexpected correlator dimensions"
                );
                let dest = c.output.join("frames").join(format!("{}.h5", meta.id));
                if dest.exists() {
                    let old = read_meta(&hdf5::File::open(&dest)?)?;
                    ensure!(
                        old.schema == SCHEMA && old.source_sha256 == meta.source_sha256,
                        "cached product mismatch"
                    );
                    return Ok(Some(meta));
                }
                let p = core::process(&data.complex(), par[21])?;
                write_frame(&c.output.join("frames"), &meta, &p, par)?;
                eprintln!(
                    "{} {}",
                    meta.id,
                    path.file_name().unwrap().to_string_lossy()
                );
                Ok(Some(meta))
            })
            .collect()
    });
    let mut count = 0;
    let mut errors = 0;
    for (path, result) in paths.iter().zip(results) {
        match result {
            Ok(Some(_)) => count += 1,
            Ok(None) => (),
            Err(e) => {
                errors += 1;
                eprintln!("ERROR {}: {e:#}", path.display());
            }
        }
    }
    println!("Node {node}/{nodes}: {count} selected records, {errors} errors");
    ensure!(
        errors == 0,
        "analysis incomplete: {errors} errors (valid products retained)"
    );
    ensure!(count > 0, "no records in requested time interval");
    Ok(())
}
fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("output parent")?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    if path.starts_with("/srv/guisdap") {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755))?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}
fn binary(path: &Path, v: &[f32]) -> Result<()> {
    let b: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    atomic_bytes(path, &b)
}
#[derive(Serialize)]
struct WebFrame {
    source_count: usize,
    id: String,
    start_unix: f64,
    end_unix: f64,
    elevation_deg: f64,
    azimuth_deg: f64,
    transmitter_w: f64,
    centers_hz: [f64; 2],
}
fn export(input: &Path, output: &Path) -> Result<()> {
    let mut frames = vec![];
    for e in fs::read_dir(input.join("frames"))? {
        let p = e?.path();
        if p.extension().is_some_and(|s| s == "h5") {
            let m = read_meta(&hdf5::File::open(&p)?)?;
            ensure!(m.schema == SCHEMA, "incompatible product schema");
            frames.push((m, p));
        }
    }
    frames.sort_by(|a, b| a.0.end_unix.total_cmp(&b.0.end_unix));
    frames.dedup_by(|a, b| a.0.source_sha256 == b.0.source_sha256);
    ensure!(!frames.is_empty(), "no HDF5 frames to export");
    for p in frames.windows(2) {
        ensure!(
            p[0].0.end_unix != p[1].0.end_unix,
            "conflicting recordings at same UTC time"
        );
    }
    let revision = format!(
        "{:x}",
        Sha256::digest(
            frames
                .iter()
                .map(|(m, _)| m.id.as_str())
                .collect::<Vec<_>>()
                .join(";")
        )
    );
    let overview_root = format!("overviews/{}/", &revision[..16]);
    let overview = output.join(&overview_root);
    let n = frames.len();
    let mut broad = vec![0f32; 800 * n];
    let mut ion = vec![0f32; 432 * n];
    let mut ion_ne = vec![0f32; 431 * n];
    let mut ion_se = vec![0f32; 431 * n];
    let mut index = vec![];
    for (t, (m, path)) in frames.iter().enumerate() {
        let f = hdf5::File::open(path)?;
        let sp = f.dataset("broad_spectrum")?.read_raw::<f32>()?;
        let power = f.dataset("ion_excess")?.read_raw::<f32>()?;
        let ne = f.dataset("ion_short_raw_ne")?.read_raw::<f32>()?;
        let se = f.dataset("ion_short_raw_ne_sigma")?.read_raw::<f32>()?;
        for i in 0..800 {
            broad[i * n + t] = sp[i];
        }
        for i in 0..432 {
            ion[i * n + t] = power[i];
        }
        for i in 0..431 {
            ion_ne[i * n + t] = ne[i];
            ion_se[i * n + t] = se[i];
        }
        let d = output.join("frames").join(&m.id);
        fs::create_dir_all(&d)?;
        for (name, file) in [
            ("native_long_acf", "native-long.f32"),
            ("native_short_acf", "native-short.f32"),
            ("native_uncoded_acf", "native-uncoded.f32"),
            ("decoded_spectrum", "decoded.f32"),
            ("plasma_excess", "plasma-power.f32"),
            ("decoded_lag0", "decoded-lag0.f32"),
        ] {
            if !d.join(file).exists() {
                binary(&d.join(file), &f.dataset(name)?.read_raw::<f32>()?)?;
            }
        }
        index.push(WebFrame {
            source_count: if f.link_exists("integration_source_count") {
                f.dataset("integration_source_count")?.read_raw::<u32>()?[0] as usize
            } else {
                1
            },
            id: m.id.clone(),
            start_unix: m.start_unix,
            end_unix: m.end_unix,
            elevation_deg: m.elevation_deg,
            azimuth_deg: m.azimuth_deg,
            transmitter_w: m.transmitter_w,
            centers_hz: m.centers_hz,
        });
    }
    // Temporal median subtraction per band/gate/frequency: diagnostic excess only.
    // Store raw broad spectra too so persistent features can be inspected.
    binary(&overview.join("broad-raw.f32"), &broad)?;
    for row in broad.chunks_mut(n) {
        let mut v = row.to_vec();
        v.sort_by(f32::total_cmp);
        let med = if n % 2 == 0 {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        } else {
            v[n / 2]
        };
        for x in row {
            *x -= med;
        }
    }
    binary(&overview.join("broad-excess.f32"), &broad)?;
    binary(&overview.join("ion-power.f32"), &ion)?;
    binary(&overview.join("ion-short-raw-ne.f32"), &ion_ne)?;
    binary(&overview.join("ion-short-raw-ne-sigma.f32"), &ion_se)?;
    let first = &frames[0].0;
    let manifest = serde_json::json!({"schema":SCHEMA,"title":"BEATA · 2 November 2022","frames":index,"overview_root":overview_root,
        "native_uncoded_shape":[2,50,4,2],"native_long_shape":[2,800,90,2],"native_short_shape":[2,50,91,2],"decoded_shape":[2,90,core::NFFT],"broad_shape":[2,4,100,n],"ion_shape":[431,n],"ion_zero_lag_shape":[432,n],
        "ion_profile":{"signal_offset":480,"gates":431,"lag_s":10e-6,"slice_offset":25531,"slices":14,"background_subtracted":false,"calibration_offset":16,"response_width_slant_km":2.1724186,"scale":"signed 10 us lag raw-Ne equivalent; peak spectral response, Te/Ti=1, no Debye correction","magic_constant":0.7,"antenna_efficiency":0.66,"uncertainty":"approximate 1-sigma scatter of fourteen subintegrations plus calibration-sample propagation; online clutter can correlate slices; excludes that covariance, decorrelation/Doppler and systematic calibration errors"},
        "lag_block_roles":{"native_long":"decoded altitude spectrum, lags 1..800", "native_short":"decoded short-lag diagnostic, distinct range response", "native_uncoded":"four overlapping broad gates; diagnostic/calibration only; excluded from altitude spectrum"},
        "decoded_fft_length":core::NFFT,"sample_interval_s":core::DT,"broad_range_bounds_km":[[47,221],[128,302],[209,383],[290,464]],
        "ion_zero_lag_range_km":(0..432).map(core::ion_range_km).collect::<Vec<_>>(),
        "ion_range_km":(0..431).map(core::ion_short_range_km).collect::<Vec<_>>(),
        "decoded_range_km":(0..90).map(core::decoded_range_km).collect::<Vec<_>>(),
        "plasma_range_km":(0..5450).map(core::plasma_range_km).collect::<Vec<_>>(),
        "nominal_elevation_deg":first.elevation_deg,
        "notes":"Quick-look relative spectra, not calibrated density. Decoded lower 12 gates are partial-code tails. The displayed ion profile uses measured 10 us lags down to ~47.5 km; broad zero-lag data remain in HDF5. Temporal median subtraction can remove persistent plasma lines; inspect raw view too."});
    // Manifest is the commit marker, published after every referenced object.
    atomic_bytes(&output.join("index.json"), &serde_json::to_vec(&manifest)?)?;
    println!("Exported {n} frames to {}", output.display());
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    match Args::parse().command {
        Command::RunStudy {
            config,
            monitor_only,
        } => study::run(&config, monitor_only),
        Command::AveragePlasma {
            input,
            output,
            integration_s,
            threads,
        } => plasma_average::run(&input, &output, integration_s, threads),
        Command::FitIon { config } => ion::run(&config),
        Command::IndexIon { config } => ion_index::run(&config),
        Command::Analyze {
            config,
            node_index,
            node_count,
        } => analyze(config, node_index, node_count),
        Command::Inspect { file } => {
            let (b, m) = load(&file)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&metadata(&file, &b, &m)?)?
            );
            for (k, a) in m {
                println!(
                    "{k}: {} x {}, complex={}",
                    a.rows,
                    a.cols,
                    !a.imag.is_empty()
                );
            }
            Ok(())
        }
        Command::Export { input, output } => export(&input, &output),
        Command::Serve { root, bind } => {
            ensure!(
                root.join("index.html").is_file(),
                "web root lacks index.html"
            );
            let root = fs::canonicalize(root)?;
            let app = axum::Router::new()
                .nest_service("/guisdap", tower_http::services::ServeDir::new(root))
                .layer(axum::middleware::from_fn(
                    |request: axum::extract::Request, next: axum::middleware::Next| async move {
                        if request.uri().path() == "/guisdap" {
                            axum::response::Redirect::permanent("/guisdap/").into_response()
                        } else {
                            next.run(request).await
                        }
                    },
                ));
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            println!("Serving http://{bind}/guisdap/");
            axum::serve(listener, app).await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod product_tests {
    use super::*;
    #[test]
    fn hdf5_and_static_export_roundtrip_and_conflict_detection() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("products");
        let mut d = vec![core::C::new(1., 0.); 196493];
        for z in &mut d[16..32] {
            z.re = 2.;
        }
        for offset in [31565, 32029, 32029 + core::STRIDE] {
            for x in &mut d[offset..offset + 16] {
                x.re = 1.;
            }
        }
        let p = core::process(&d, 1.)?;
        let mut meta = FrameMeta {
            id: "fixture".into(),
            source: "private/source.mat".into(),
            source_sha256: "abcdef".into(),
            start_unix: 1.,
            end_unix: 6.,
            elevation_deg: 77.8,
            azimuth_deg: 189.,
            centers_hz: [-2.5e6, -4.9e6],
            integration_s: 5.,
            transmitter_w: 1e6,
            schema: SCHEMA.into(),
        };
        write_frame(&output.join("frames"), &meta, &p, &[896.; 67])?;
        write_frame(&output.join("frames"), &meta, &p, &[896.; 67])?; // Idempotent publication.
        let f = hdf5::File::open(output.join("frames/fixture.h5"))?;
        assert_eq!(f.dataset("decoded_spectrum")?.shape(), [2, 90, core::NFFT]);
        assert_eq!(f.dataset("ion_excess")?.read_raw::<f32>()?, p.ion_excess);
        drop(f);
        export(&output, &dir.path().join("web"))?;
        assert_eq!(
            fs::metadata(dir.path().join("web/frames/fixture/decoded.f32"))?.len(),
            (2 * 90 * core::NFFT * 4) as u64
        );
        let index = fs::read_to_string(dir.path().join("web/index.json"))?;
        assert!(!index.contains("private/source"));
        meta.source_sha256 = "conflict".into();
        assert!(write_frame(&output.join("frames"), &meta, &p, &[896.; 67]).is_err());
        Ok(())
    }
}
