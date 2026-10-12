//! Minimal ion inputs: validated record identity and signed raw-Ne products.
use super::*;
use std::sync::Mutex;
pub fn run(path: &Path) -> Result<()> {
    let cfg: Config = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        cfg.threads > 0 && !cfg.roots.is_empty(),
        "invalid capacity or roots"
    );
    let start = chrono::DateTime::parse_from_rfc3339(&cfg.start)?.timestamp() as f64;
    let end = chrono::DateTime::parse_from_rfc3339(&cfg.end)?.timestamp() as f64;
    let mut paths = vec![];
    for root in &cfg.roots {
        ensure!(root.is_dir(), "missing data root {}", root.display());
        for e in walkdir::WalkDir::new(root).follow_links(false) {
            let e = e?;
            let name = e.file_name().to_string_lossy();
            if e.file_type().is_file()
                && (name.ends_with(".mat.bz2") || name.ends_with(".mat"))
                && name.as_bytes()[0].is_ascii_digit()
            {
                paths.push(e.path().to_path_buf());
            }
        }
    }
    paths.sort();
    paths.dedup();
    ensure!(!paths.is_empty(), "no source files");
    fs::create_dir_all(cfg.output.join("frames"))?;
    let progress = Mutex::new((0usize, Vec::<String>::new()));
    let publish = |completed: usize, errors: &[String]| -> Result<()> {
        atomic_bytes(
            &cfg.output.join("index-status.json"),
            &serde_json::to_vec(
                &serde_json::json!({"phase":"ion_input_index","threads":cfg.threads,"total":paths.len(),"completed":completed,"errors":errors,"updated_unix":Utc::now().timestamp()}),
            )?,
        )
    };
    publish(0, &[])?;
    rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.threads)
        .build()?
        .install(|| {
            paths.par_iter().for_each(|path| {
                let result = (|| -> Result<()> {
                    let (bytes, m) = load(path)?;
                    let par = &m.get("d_parbl").context("missing parameters")?.real;
                    let middle = timestamp(par)? - par[6] / 2.;
                    if middle < start || middle >= end {
                        return Ok(());
                    }
                    let mut meta = metadata(path, &bytes, &m)?;
                    meta.schema = "beata22-ion-input-v1".into();
                    let dest = cfg.output.join("frames").join(format!("{}.h5", meta.id));
                    if dest.exists() {
                        ensure!(
                            read_meta(&hdf5::File::open(&dest)?)?.source_sha256
                                == meta.source_sha256,
                            "input hash changed"
                        );
                        return Ok(());
                    }
                    let raw = core::process_ion(
                        &m.get("d_data").context("missing correlator")?.complex(),
                        par[21],
                    )?;
                    let tmp = tempfile::NamedTempFile::new_in(cfg.output.join("frames"))?;
                    {
                        let h = hdf5::File::create(tmp.path())?;
                        h.new_dataset_builder()
                            .with_data(serde_json::to_vec(&meta)?.as_slice())
                            .create("metadata_json")?;
                        h.new_dataset_builder().with_data(par).create("d_parbl")?;
                        for (prefix, n, values, range) in [
                            (
                                "ion_short",
                                431,
                                core::ion_short_raw_ne(&raw, par[20], meta.transmitter_w, par[63]),
                                core::ion_short_range_km as fn(usize) -> f64,
                            ),
                            (
                                "ion",
                                432,
                                core::ion_raw_ne(&raw, par[20], meta.transmitter_w, par[63]),
                                core::ion_range_km as fn(usize) -> f64,
                            ),
                        ] {
                            if let Ok((ne, se)) = values {
                                dataset(
                                    &h,
                                    &format!("{prefix}_raw_ne"),
                                    &[n],
                                    &ne,
                                    "m^-3; signed raw-Ne equivalent",
                                )?;
                                dataset(
                                    &h,
                                    &format!("{prefix}_raw_ne_sigma"),
                                    &[n],
                                    &se,
                                    "m^-3; approximate standard error",
                                )?;
                                let altitude: Vec<_> = (0..n)
                                    .map(|i| core::altitude_km(range(i), meta.elevation_deg))
                                    .collect();
                                h.new_dataset_builder()
                                    .with_data(&altitude)
                                    .create(format!("{prefix}_altitude_km").as_str())?;
                            }
                        }
                        h.flush()?;
                    }
                    tmp.persist(dest).map_err(|e| e.error)?;
                    Ok(())
                })();
                let mut p = progress.lock().unwrap();
                p.0 += 1;
                if let Err(e) = result {
                    p.1.push(format!("{}: {e:#}", path.display()));
                }
                if p.0.is_multiple_of(32) || p.0 == paths.len() {
                    let _ = publish(p.0, &p.1);
                    eprintln!(
                        "Indexed {}/{} records; {} errors",
                        p.0,
                        paths.len(),
                        p.1.len()
                    );
                }
            })
        });
    let p = progress.into_inner().unwrap();
    publish(p.0, &p.1)?;
    ensure!(
        p.1.is_empty(),
        "{} input errors; see index-status.json",
        p.1.len()
    );
    Ok(())
}
