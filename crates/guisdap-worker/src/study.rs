//! One configurable 128-thread budget, atomic publication, and read-only status.
use super::*;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Study {
    dataset: String,
    data_sources: Vec<PathBuf>,
    start: String,
    end: String,
    worker_threads: usize,
    work_root: PathBuf,
    publication_root: PathBuf,
    fit_template: PathBuf,
    integrations_s: Vec<u32>,
    gui_config: PathBuf,
}
fn count(path: &Path) -> usize {
    fs::read_dir(path)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "h5"))
                .count()
        })
        .unwrap_or(0)
}
fn ion_name(c: &Study, dt: u32) -> String {
    format!("{}-ionfit-full-{dt}s-v2", c.dataset)
}
fn plasma_name(c: &Study, dt: u32) -> String {
    format!("{}-plasma-full-{dt}s-v1", c.dataset)
}
fn source_count(c: &Study) -> Result<usize> {
    let mut n = 0;
    for root in &c.data_sources {
        ensure!(root.is_dir(), "data source not mounted: {}", root.display());
        for e in walkdir::WalkDir::new(root) {
            let e = e?;
            let name = e.file_name().to_string_lossy();
            if e.file_type().is_file()
                && (name.ends_with(".mat.bz2") || name.ends_with(".mat"))
                && name.as_bytes()[0].is_ascii_digit()
            {
                n += 1;
            }
        }
    }
    Ok(n)
}
fn publish(c: &Study, total: usize) -> Result<bool> {
    let inputs = count(&c.work_root.join(format!("{}-ion-inputs/frames", c.dataset)));
    let native = count(&c.work_root.join(format!("{}-full-v5/frames", c.dataset)));
    let mut stages = vec![
        json!({"name":"Input validation","completed":inputs,"total":total,"state":if inputs==total{"complete"}else{"processing"}}),
    ];
    for dt in &c.integrations_s {
        let root = c.publication_root.join(ion_name(c, *dt));
        let status = fs::read(root.join("status.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .or_else(|| {
                fs::read(root.join("index.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .map(|v| v["processing"].clone())
            });
        let p = status.unwrap_or(json!({"phase":"queued","completed":0,"total":0}));
        stages.push(json!({"name":format!("Ion fits · {dt} s"),"completed":p["completed"],"total":p["total"],"state":p["phase"],"failed":p["failed"],"insufficient_records":p["insufficient"]}));
    }
    stages.push(json!({"name":"Native plasma-line products","completed":native,"total":total,"state":if native==total{"complete"}else{"processing"}}));
    for dt in &c.integrations_s {
        let available = count(&c.work_root.join(plasma_name(c, *dt)).join("frames"));
        let exported = fs::read(
            c.publication_root
                .join(plasma_name(c, *dt))
                .join("index.json"),
        )
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["frames"].as_array().map(Vec::len))
        .unwrap_or(0);
        let expected = stages
            .iter()
            .find(|s| s["name"] == format!("Ion fits · {dt} s"))
            .and_then(|s| s["total"].as_u64())
            .unwrap_or(0) as usize;
        stages.push(json!({"name":format!("Plasma-line averages · {dt} s"),"completed":available,"published":exported,"total":expected,"state":if native==total&&expected>0&&available>=expected&&exported==available{"complete"}else if available>0{"processing"}else{"queued"}}));
    }
    let complete = stages.iter().all(|s| s["state"] == "complete");
    let data = json!({"dataset":c.dataset,"state":if complete{"complete"}else{"processing"},"configured_worker_threads":c.worker_threads,"source_records":total,"updated_unix":Utc::now().timestamp(),"stages":stages});
    atomic_bytes(
        &c.publication_root.join("study-status.json"),
        &serde_json::to_vec(&data)?,
    )?;
    Ok(complete)
}
pub fn run(path: &Path, monitor_only: bool) -> Result<()> {
    let c: Study = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        c.worker_threads > 0
            && !c.integrations_s.is_empty()
            && c.integrations_s.iter().all(|dt| [15, 60].contains(dt)),
        "invalid study configuration"
    );
    let total = source_count(&c)?;
    ensure!(total > 0, "empty source inventory");
    fs::create_dir_all(&c.publication_root)?;
    fs::create_dir_all(c.work_root.join("run-config"))?;
    if monitor_only {
        loop {
            if publish(&c, total)? {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let result = std::thread::scope(|scope| -> Result<()> {
        let stop2 = stop.clone();
        let reference = &c;
        scope.spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                let _ = publish(reference, total);
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        });
        let outcome = (|| -> Result<()> {
            let inputs = c.work_root.join(format!("{}-ion-inputs", c.dataset));
            let config = Config {
                roots: c.data_sources.clone(),
                output: inputs.clone(),
                start: c.start.clone(),
                end: c.end.clone(),
                threads: c.worker_threads,
            };
            let path = c.work_root.join("run-config/inputs.json");
            atomic_bytes(&path, &serde_json::to_vec(&config)?)?;
            ion_index::run(&path)?;
            for dt in &c.integrations_s {
                let mut fit: ion::Config = serde_json::from_slice(&fs::read(&c.fit_template)?)?;
                fit.quicklook_frames = inputs.join("frames");
                fit.output = c.work_root.join(ion_name(&c, *dt));
                fit.export = c.publication_root.join(ion_name(&c, *dt));
                fit.live_export = None;
                fit.integration_s = *dt as f64;
                fit.threads = c.worker_threads;
                fit.streaming = true;
                let path = c.work_root.join(format!("run-config/ion-{dt}.json"));
                atomic_bytes(&path, &serde_json::to_vec(&fit)?)?;
                ion::run(&path)?;
            }
            let native = c.work_root.join(format!("{}-full-v5", c.dataset));
            let config = Config {
                output: native.clone(),
                ..config
            };
            let path = c.work_root.join("run-config/plasma.json");
            atomic_bytes(&path, &serde_json::to_vec(&config)?)?;
            analyze(path, 0, 1)?;
            for dt in &c.integrations_s {
                let output = c.work_root.join(plasma_name(&c, *dt));
                plasma_average::run(&native, &output, *dt as f64, c.worker_threads)?;
                export(&output, &c.publication_root.join(plasma_name(&c, *dt)))?;
                let datasets:Vec<_>=c.integrations_s.iter().filter(|dt|c.publication_root.join(plasma_name(&c,**dt)).join("index.json").exists()).map(|dt|json!({"label":format!("{dt} seconds"),"data_root":format!("data/{}/",plasma_name(&c,*dt))})).collect();
                let cfg = json!({"data_root":format!("data/{}/",plasma_name(&c,*dt)),"datasets":datasets});
                atomic_bytes(&c.gui_config, &serde_json::to_vec(&cfg)?)?;
            }
            Ok(())
        })();
        stop.store(true, Ordering::Relaxed);
        outcome
    });
    publish(&c, total)?;
    result
}
