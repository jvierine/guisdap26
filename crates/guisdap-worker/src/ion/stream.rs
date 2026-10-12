//! Atomic, resumable interval products and a small, live browser index.
use super::*;
use serde_json::{json, Value};
use std::{sync::mpsc, time::Instant};
#[path = "stream_hdf5.rs"]
mod store;
#[derive(Deserialize)]
struct SeedIndex {
    profiles: Vec<SeedProfile>,
}
#[derive(Deserialize)]
struct SeedProfile {
    start_unix: f64,
    end_unix: f64,
    gates: Vec<SeedGate>,
}
#[derive(Deserialize)]
struct SeedGate {
    result: Option<SeedFit>,
}
#[derive(Deserialize)]
struct SeedFit {
    plasma: PlasmaState,
}

fn blank(cfg: &Config, lo: f64, hi: f64, status: &str) -> Gate {
    let h = (lo + hi) / 2.;
    Gate {
        altitude_km: h,
        lower_km: lo,
        upper_km: hi,
        response_width_max_km: 0.,
        first_lag_signal_to_noise: None,
        oxygen_fraction: 1.
            / (1. + (-(h - cfg.composition_transition_km) / cfg.composition_scale_km).exp()),
        model: SpectrumModel {
            radar_frequency_hz: 930e6,
            ions: vec![],
            electron_collision_s: 0.,
        },
        settings: FitSettings {
            tie_electron_ion_temperature: h < cfg.tie_temperature_below_km,
            fit_collision_rate: h < cfg.fit_collisions_below_km,
            ..Default::default()
        },
        result: None,
        status: status.into(),
        lag_us: vec![],
        observed: vec![],
        sigma: vec![],
        addresses: vec![],
    }
}
fn placeholder(cfg: &Config, window: i64, metas: Vec<FrameMeta>) -> Profile {
    Profile {
        start_unix: window as f64 * cfg.integration_s,
        end_unix: (window + 1) as f64 * cfg.integration_s,
        source_count: metas.len(),
        sources: metas,
        gates: cfg
            .altitude_edges_km
            .windows(2)
            .map(|h| blank(cfg, h[0], h[1], "pending"))
            .collect(),
        processing_state: "pending".into(),
        detail_url: None,
        raw_ne: vec![],
        notes: vec![],
    }
}
fn raw_ne(cfg: &Config, metas: &[FrameMeta]) -> Result<Vec<Option<[f64; 2]>>> {
    let mut samples = vec![vec![]; cfg.altitude_edges_km.len() - 1];
    let mut noise = vec![vec![]; samples.len()];
    for m in metas {
        let h = hdf5::File::open(cfg.quicklook_frames.join(format!("{}.h5", m.id)))?;
        if !h.link_exists("ion_short_raw_ne") {
            continue;
        }
        let alt = h.dataset("ion_short_altitude_km")?.read_raw::<f64>()?;
        let ne = h.dataset("ion_short_raw_ne")?.read_raw::<f32>()?;
        let se = h.dataset("ion_short_raw_ne_sigma")?.read_raw::<f32>()?;
        let factor = cfg.calibration_multiplier / 0.7 * 0.66 / cfg.antenna_efficiency;
        for (i, edges) in cfg.altitude_edges_km.windows(2).enumerate() {
            let bounds = cfg
                .native_slant_edges_km
                .get(i..i + 2)
                .map(|r| {
                    [
                        altitude_km(r[0], m.elevation_deg),
                        altitude_km(r[1], m.elevation_deg),
                    ]
                })
                .unwrap_or([edges[0], edges[1]]);
            let cells: Vec<_> = (0..alt.len())
                .filter(|&j| alt[j] >= bounds[0] - 1e-8 && alt[j] < bounds[1] - 1e-8)
                .collect();
            if cells.is_empty() {
                continue;
            }
            let n = cells.len() as f64;
            samples[i].push(cells.iter().map(|&j| ne[j] as f64 * factor).sum::<f64>() / n);
            // Conservative range-correlated propagation; no independent-gate gain assumed.
            noise[i].push((cells.iter().map(|&j| se[j] as f64 * factor).sum::<f64>() / n).powi(2));
        }
    }
    Ok(samples
        .iter()
        .zip(noise)
        .map(|(s, noise)| {
            if s.is_empty() {
                return None;
            }
            let n = s.len() as f64;
            let mean = s.iter().sum::<f64>() / n;
            let propagated = noise.iter().sum::<f64>() / (n * n);
            let temporal = if s.len() > 1 {
                s.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n * (n - 1.))
            } else {
                0.
            };
            Some([mean, propagated.max(temporal).sqrt()])
        })
        .collect())
}
fn compact(profile: &Profile) -> Result<Value> {
    let mut p = serde_json::to_value(profile)?;
    p.as_object_mut().unwrap().remove("sources");
    for g in p["gates"].as_array_mut().unwrap() {
        let obj = g.as_object_mut().unwrap();
        for key in ["model", "addresses", "observed", "sigma", "lag_us"] {
            obj.remove(key);
        }
        if let Some(r) = obj.get_mut("result").and_then(Value::as_object_mut) {
            for key in [
                "predicted",
                "standardized_residual",
                "model_curve",
                "covariance",
                "conditional_standard_error",
                "free_parameter_indices",
            ] {
                r.remove(key);
            }
        }
    }
    Ok(p)
}
fn public_detail(profile: &Profile) -> Result<Vec<u8>> {
    let mut p = serde_json::to_value(profile)?;
    p.as_object_mut().unwrap().remove("sources");
    for g in p["gates"].as_array_mut().unwrap() {
        g.as_object_mut().unwrap().remove("addresses");
    }
    Ok(serde_json::to_vec(&p)?)
}
fn publish(cfg: &Config, profiles: &[Value], phase: &str, started: i64) -> Result<()> {
    let completed = profiles
        .iter()
        .filter(|p| p["processing_state"] != "pending")
        .count();
    let failed = profiles
        .iter()
        .filter(|p| p["processing_state"] == "failed")
        .count();
    let insufficient = profiles
        .iter()
        .filter(|p| p["processing_state"] == "insufficient_records")
        .count();
    let mut document = json!({"schema":"beata22-ionfit-v2","integration_s":cfg.integration_s,
        "uncertainty":"Temporal record-cluster covariance with conditional variance floor; few-record windows have noisy uncertainty estimates. Fixed calibration and composition errors excluded.",
        "limitations":["Maxwellian local homogeneous fit; fixed altitude-dependent ion composition", "All native decoded ion lags retained; no spline filling or SVD", "Raw Ne panel uses signed 10 microsecond lag, range-squared correction, Te/Ti=1 and peak spectral response; not fitted density or zero lag", "Missing-record intervals are explicit; no temporal interpolation"],
        "processing":{"phase":phase,"threads":cfg.threads,"total":profiles.len(),"completed":completed,"failed":failed,"insufficient":insufficient,"source_records":profiles.iter().map(|p|p["source_count"].as_u64().unwrap_or(0)).sum::<u64>(),"started_unix":started,"updated_unix":chrono::Utc::now().timestamp()},
        "profiles":profiles});
    document["composition"] = json!({"model":"fixed_logistic_Oplus_molecular","transition_km":cfg.composition_transition_km,"scale_km":cfg.composition_scale_km,"molecular_mass_amu":30.5,"iri":false});
    document["warm_start_index"] = json!(cfg.warm_start_index);
    document["optimizer"] = json!({"version":"hardened-v3","max_iterations":160,"warm_starts":"same gate, previous time, neighboring altitudes; original two starts retained","rescue":"four independent temperature/velocity/collision seeds on failed, boundary, low-Ti, or high-residual fits"});
    document["model_selection_below_km"] = json!(cfg.model_selection_below_km);
    if cfg.model_selection_below_km.is_some() {
        document["limitations"].as_array_mut().unwrap().push(json!("Below the configured model-selection altitude, compare free/tied temperatures and free/zero collision models using chi-square + parameter_count*ln(real_components). Conditional diagonal-noise criterion, not a calibrated probability; selected constraints are stored per gate."));
    }
    let bytes = serde_json::to_vec(&document)?;
    document["calibration"] = json!({"magic_constant":cfg.calibration_multiplier,"antenna_efficiency":cfg.antenna_efficiency});
    let status = serde_json::to_vec(&document["processing"])?;
    super::super::atomic_bytes(&cfg.export.join("status.json"), &status)?;
    if let Some(live) = &cfg.live_export {
        super::super::atomic_bytes(&live.join("status.json"), &status)?;
    }
    super::super::atomic_bytes(&cfg.export.join("index.json"), &bytes)?;
    if let Some(live) = &cfg.live_export {
        super::super::atomic_bytes(&live.join("index.json"), &bytes)?;
    }
    Ok(())
}
pub fn run(cfg: &Config) -> Result<()> {
    ensure!(
        cfg.integration_s >= 15. && cfg.integration_s.is_finite() && cfg.threads > 0,
        "invalid integration/capacity"
    );
    ensure!(
        cfg.altitude_edges_km.len() > 1
            && cfg
                .altitude_edges_km
                .windows(2)
                .all(|h| h[0].is_finite() && h[1].is_finite() && h[1] > h[0]),
        "invalid altitude grid"
    );
    let exp = Experiment::read(&cfg.experiment_model)?;
    let seed_bytes = cfg.warm_start_index.as_ref().map(fs::read).transpose()?;
    let seed_hash = seed_bytes
        .as_ref()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
    let seed_index = seed_bytes
        .as_ref()
        .map(|bytes| serde_json::from_slice::<SeedIndex>(bytes))
        .transpose()?;
    if let Some(index) = &seed_index {
        ensure!(
            index
                .profiles
                .iter()
                .all(|p| p.gates.len() + 1 == cfg.altitude_edges_km.len()
                    && (p.end_unix - p.start_unix - cfg.integration_s).abs() < 0.01),
            "warm-start grid or cadence differs"
        );
    }

    let mut windows: BTreeMap<i64, Vec<FrameMeta>> = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for e in fs::read_dir(&cfg.quicklook_frames)? {
        let path = e?.path();
        if !path.extension().is_some_and(|x| x == "h5") {
            continue;
        }
        let m = super::super::read_meta(&hdf5::File::open(path)?)?;
        ensure!(
            seen.insert(m.source_sha256.clone()),
            "duplicate source recording"
        );
        windows
            .entry(((m.start_unix + m.end_unix) / 2. / cfg.integration_s).floor() as i64)
            .or_default()
            .push(m);
    }
    ensure!(!windows.is_empty(), "no source records");
    let first = *windows.keys().next().unwrap();
    let last = *windows.keys().last().unwrap();
    let mut templates = vec![];
    for key in first..=last {
        let mut metas = windows.remove(&key).unwrap_or_default();
        metas.sort_by(|a, b| a.start_unix.total_cmp(&b.start_unix));
        ensure!(
            metas
                .windows(2)
                .all(|w| w[1].start_unix >= w[0].end_unix - 0.01),
            "overlapping records"
        );
        templates.push(placeholder(cfg, key, metas));
    }
    fs::create_dir_all(cfg.output.join("profiles"))?;
    let started = chrono::Utc::now().timestamp();
    let mut overview = templates.iter().map(compact).collect::<Result<Vec<_>>>()?;
    publish(cfg, &overview, "running", started)?;
    let mut fingerprint = serde_json::to_value(cfg)?;
    for key in ["threads", "output", "export", "live_export"] {
        fingerprint.as_object_mut().unwrap().remove(key);
    }
    let fingerprint = format!(
        "stream-hardened-v3:{}:{:?}:{}",
        exp.hash,
        seed_hash,
        serde_json::to_string(&fingerprint)?
    );
    let (tx, rx) = mpsc::sync_channel::<(usize, Result<Value>)>(8);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.threads)
        .build()?;
    let processing = std::thread::scope(|scope| -> Result<()> {
        scope.spawn(|| {
            pool.install(|| {
                templates
                    .par_iter()
                    .enumerate()
                    .for_each_with(tx, |tx, (i, template)| {
                        let outcome = (|| -> Result<Value> {
                            let key = format!(
                                "{:x}",
                                Sha256::digest(format!(
                                    "{}:{}",
                                    fingerprint,
                                    serde_json::to_string(&template.sources)?
                                ))
                            );
                            let name = format!("{:.0}", template.start_unix);
                            let dest = cfg.output.join("profiles").join(format!("{name}.h5"));
                            let cached = if dest.exists() {
                                let h = hdf5::File::open(&dest)?;
                                if h.dataset("cache_key")?.read_raw::<u8>()? == key.as_bytes() {
                                    Some(serde_json::from_slice::<Profile>(
                                        &h.dataset("profile_json")?.read_raw::<u8>()?,
                                    )?)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let mut p = if let Some(p) = cached {
                                p
                            } else {
                                let mut p = template.clone();
                                p.raw_ne = raw_ne(cfg, &p.sources)?;
                                if p.source_count < 3 {
                                    p.processing_state = "insufficient_records".into();
                                    for g in &mut p.gates {
                                        g.status = "insufficient_records".into();
                                    }
                                } else {
                                    let result = (|| -> Result<Vec<Gate>> {
                                        ensure!(
                                            p.sources.iter().all(|m| (m.elevation_deg
                                                - p.sources[0].elevation_deg)
                                                .abs()
                                                < 0.1
                                                && (m.azimuth_deg - p.sources[0].azimuth_deg)
                                                    .abs()
                                                    < 0.1),
                                            "mixed beam directions"
                                        );
                                        let records = p
                                            .sources
                                            .iter()
                                            .map(|m| read_record(m, &exp, cfg))
                                            .collect::<Result<Vec<_>>>()?;
                                        if let Some(index) = &seed_index {
                                            let same = index.profiles.iter().find(|q| {
                                                (q.start_unix - p.start_unix).abs() < 0.01
                                            });
                                            let previous = index
                                                .profiles
                                                .iter()
                                                .find(|q| (q.end_unix - p.start_unix).abs() < 0.01);
                                            let mut gates: Vec<Gate> = Vec::new();
                                            for (j, h) in
                                                cfg.altitude_edges_km.windows(2).enumerate()
                                            {
                                                let mut seeds = Vec::new();
                                                for (profile, k) in [
                                                    (same, j),
                                                    (previous, j),
                                                    (same, j.saturating_sub(1)),
                                                    (same, j + 1),
                                                ] {
                                                    if let Some(f) = profile
                                                        .and_then(|q| q.gates.get(k))
                                                        .and_then(|g| g.result.as_ref())
                                                    {
                                                        seeds.push(f.plasma);
                                                    }
                                                }
                                                if let Some(f) =
                                                    gates.last().and_then(|g| g.result.as_ref())
                                                {
                                                    seeds.push(f.plasma);
                                                }
                                                gates.push(fit_gate(
                                                    &records, &exp, cfg, h[0], h[1], &seeds,
                                                )?);
                                            }
                                            Ok(gates)
                                        } else {
                                            cfg.altitude_edges_km
                                                .par_windows(2)
                                                .map(|h| {
                                                    fit_gate(&records, &exp, cfg, h[0], h[1], &[])
                                                })
                                                .collect()
                                        }
                                    })();
                                    match result {
                                        Ok(gates) => {
                                            p.gates = gates;
                                            p.processing_state = "complete".into();
                                        }
                                        Err(e) => {
                                            p.processing_state = "failed".into();
                                            p.notes.push(format!("{e:#}"));
                                            for g in &mut p.gates {
                                                g.status = "processing_failed".into();
                                            }
                                        }
                                    }
                                }
                                if (p.source_count as f64 * 5. - cfg.integration_s).abs() > 0.01 {
                                    p.notes.push(format!(
                                        "{} of {:.0} expected five-second records present",
                                        p.source_count,
                                        cfg.integration_s / 5.
                                    ));
                                }
                                p.detail_url = Some(format!("profiles/{name}-{}.json", &key[..16]));
                                store::write(&dest, &p, &key, &exp.frequency_hz)?;
                                p
                            };
                            p.detail_url = Some(format!("profiles/{name}-{}.json", &key[..16]));
                            let detail = public_detail(&p)?;
                            super::super::atomic_bytes(
                                &cfg.export.join(p.detail_url.as_ref().unwrap()),
                                &detail,
                            )?;
                            if let Some(live) = &cfg.live_export {
                                super::super::atomic_bytes(
                                    &live.join(p.detail_url.as_ref().unwrap()),
                                    &detail,
                                )?;
                            }
                            compact(&p)
                        })();
                        let _ = tx.send((i, outcome));
                    })
            });
        });
        let mut last_publish = Instant::now();
        let mut done = 0;
        for (i, outcome) in rx {
            overview[i] = match outcome {
                Ok(p) => p,
                Err(e) => {
                    let mut p = templates[i].clone();
                    p.processing_state = "failed".into();
                    p.notes.push(format!("{e:#}"));
                    compact(&p)?
                }
            };
            done += 1;
            if last_publish.elapsed().as_secs() >= 5 || done == overview.len() {
                publish(cfg, &overview, "running", started)?;
                last_publish = Instant::now();
                eprintln!(
                    "{}s fits: {done}/{} intervals; {} threads",
                    cfg.integration_s,
                    overview.len(),
                    cfg.threads
                );
            }
        }
        Ok(())
    });
    processing?;
    let failed = overview
        .iter()
        .filter(|p| p["processing_state"] == "failed")
        .count();
    publish(
        cfg,
        &overview,
        if failed == 0 {
            "complete"
        } else {
            "complete_with_errors"
        },
        started,
    )?;
    let manifest = serde_json::to_vec(
        &json!({"config":cfg,"experiment_sha256":exp.hash,"profiles":overview}),
    )?;
    let tmp = tempfile::NamedTempFile::new_in(&cfg.output)?;
    {
        let h = hdf5::File::create(tmp.path())?;
        h.new_dataset_builder()
            .with_data(&manifest)
            .create("manifest_json")?;
        h.flush()?;
    }
    tmp.persist(cfg.output.join("index.h5"))
        .map_err(|e| e.error)?;
    ensure!(
        failed == 0,
        "{failed} failed intervals; inspect processing status"
    );
    Ok(())
}
