//! Private capability-addressed calibration notebooks with immutable HDF5 revisions.
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
struct Store {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}
#[derive(Serialize, Deserialize, Default)]
struct Notebook {
    revision: u64,
    points: Vec<serde_json::Value>,
}
fn valid(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|c| c.is_ascii_hexdigit())
}
fn load(s: &Store, key: &str) -> Result<Notebook, StatusCode> {
    let path = s.root.join(key).join("latest.json");
    match std::fs::read(path) {
        Ok(v) => serde_json::from_slice(&v).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Notebook::default()),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
async fn read(
    State(s): State<Store>,
    Path(key): Path<String>,
) -> Result<Json<Notebook>, StatusCode> {
    if !valid(&key) {
        return Err(StatusCode::NOT_FOUND);
    }
    let _guard = s
        .lock
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    load(&s, &key).map(Json)
}
async fn write(
    State(s): State<Store>,
    Path(key): Path<String>,
    Json(mut n): Json<Notebook>,
) -> Result<Json<Notebook>, StatusCode> {
    if !valid(&key) || n.points.len() > 10000 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let _guard = s
        .lock
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let old = load(&s, &key)?;
    if old.revision != n.revision {
        return Err(StatusCode::CONFLICT);
    }
    n.revision += 1;
    let persist = || -> anyhow::Result<()> {
        let dir = s.root.join(&key);
        std::fs::create_dir_all(&dir)?;
        let bytes = serde_json::to_vec_pretty(&n)?;
        // Preserve every edit, including removed points, for recovery and provenance.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let file =
            hdf5::File::create_excl(dir.join(format!("revision-{:06}-{stamp}.h5", n.revision)))?;
        file.new_dataset_builder()
            .with_data(bytes.as_slice())
            .create("notebook_json_utf8")?;
        file.flush()?;
        drop(file);
        let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
        use std::io::Write;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(dir.join("latest.json"))?;
        Ok(())
    };
    persist().map_err(|e| {
        eprintln!("calibration save failed: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(n))
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(std::env::var("GUISDAP_CALIBRATION_ROOT")?);
    std::fs::create_dir_all(&root)?;
    let app = Router::new()
        .route("/guisdap/api/calibrations/{key}", get(read).put(write))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(Store {
            root,
            lock: Arc::new(Mutex::new(())),
        });
    axum::serve(tokio::net::TcpListener::bind("127.0.0.1:8091").await?, app).await?;
    Ok(())
}
