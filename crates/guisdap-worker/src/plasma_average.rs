//! Average signed normalized ACFs before their linear FFT; retain every native lag.
use super::*;
pub fn run(input: &Path, output: &Path, integration: f64, threads: usize) -> Result<()> {
    ensure!(
        [15., 60.].contains(&integration) && threads > 0,
        "unsupported plasma integration/capacity"
    );
    let mut windows: BTreeMap<i64, Vec<(FrameMeta, PathBuf)>> = BTreeMap::new();
    for e in fs::read_dir(input.join("frames"))? {
        let path = e?.path();
        if !path.extension().is_some_and(|s| s == "h5") {
            continue;
        }
        let m = read_meta(&hdf5::File::open(&path)?)?;
        ensure!(m.schema == SCHEMA, "wrong input schema");
        windows
            .entry(((m.start_unix + m.end_unix) / 2. / integration).floor() as i64)
            .or_default()
            .push((m, path));
    }
    ensure!(!windows.is_empty(), "no plasma products");
    fs::create_dir_all(output.join("frames"))?;
    let results:Vec<Result<()>>=rayon::ThreadPoolBuilder::new().num_threads(threads).build()?.install(||windows.par_iter().map(|(window,frames)| {
        let mut frames=frames.clone();frames.sort_by(|a,b|a.0.start_unix.total_cmp(&b.0.start_unix));
        ensure!(frames.iter().all(|(m,_)|(m.elevation_deg-frames[0].0.elevation_deg).abs()<0.1&&m.centers_hz==frames[0].0.centers_hz),"mixed beam/tuning in plasma interval");
        let sources:Vec<_>=frames.iter().map(|(m,_)|m).collect();
        let hash=format!("{:x}",Sha256::digest(serde_json::to_vec(&sources)?));
        let end=(*window+1) as f64*integration;let mut meta=frames[0].0.clone();
        meta.id=format!("{}-{}",(end*1e6).round() as i64,&hash[..16]);meta.source=format!("mean of {} native records",frames.len());meta.source_sha256=hash;
        meta.start_unix=*window as f64*integration;meta.end_unix=end;meta.integration_s=integration;meta.transmitter_w=frames.iter().map(|(m,_)|m.transmitter_w).sum::<f64>()/frames.len() as f64;
        let dest=output.join("frames").join(format!("{}.h5",meta.id));if dest.exists(){return Ok(());}
        let mut arrays:BTreeMap<String,(Vec<usize>,Vec<f64>)>=BTreeMap::new();
        for (_,path) in &frames {let h=hdf5::File::open(path)?;
            for name in ["native_long_acf","native_short_acf","native_uncoded_acf","broad_spectrum","decoded_spectrum","decoded_lag0","ion_excess","ion_short_raw_ne","ion_short_raw_ne_sigma","ion_raw_ne","ion_raw_ne_sigma","plasma_excess"] {
                let ds=h.dataset(name)?;let v=ds.read_raw::<f32>()?;let entry=arrays.entry(name.into()).or_insert_with(||(ds.shape(),vec![0.;v.len()]));ensure!(entry.1.len()==v.len(),"shape change");
                for (sum,x) in entry.1.iter_mut().zip(v){*sum+=if name.ends_with("sigma"){(x as f64).powi(2)}else{x as f64};}
            }
        }
        let tmp=tempfile::NamedTempFile::new_in(output.join("frames"))?;
        {let h=hdf5::File::create(tmp.path())?;
            h.new_dataset_builder().with_data(serde_json::to_vec(&meta)?.as_slice()).create("metadata_json")?;
            h.new_dataset_builder().with_data(serde_json::to_vec(&sources)?.as_slice()).create("source_records_json")?;
            h.new_dataset_builder().with_data(&[frames.len() as u32]).create("integration_source_count")?;
            for (name,(shape,sums)) in arrays {let values:Vec<f32>=sums.iter().map(|sum|(if name.ends_with("sigma"){sum.sqrt()}else{*sum}/frames.len() as f64) as f32).collect();dataset(&h,&name,&shape,&values,if name.contains("raw_ne"){"m^-3; signed temporal mean; independent-record propagated sigma"}else{"signed temporal mean of normalized native products"})?;}
            h.flush()?;
        }tmp.persist(dest).map_err(|e|e.error)?;Ok(())
    }).collect());
    for result in results {
        result?;
    }
    eprintln!(
        "Averaged {} plasma intervals at {integration}s",
        windows.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_windows_preserve_signed_means_and_propagate_sigma() {
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        fs::create_dir(input.path().join("frames")).unwrap();
        for i in 0..2 {
            let h =
                hdf5::File::create(input.path().join("frames").join(format!("{i}.h5"))).unwrap();
            let m = FrameMeta {
                id: format!("source{i}"),
                source: format!("record{i}"),
                source_sha256: format!("hash{i}"),
                start_unix: i as f64 * 5.,
                end_unix: (i + 1) as f64 * 5.,
                integration_s: 5.,
                elevation_deg: 77.8,
                azimuth_deg: 180.,
                centers_hz: [-2.5e6, -4.9e6],
                transmitter_w: 1e6,
                schema: SCHEMA.into(),
            };
            h.new_dataset_builder()
                .with_data(serde_json::to_vec(&m).unwrap().as_slice())
                .create("metadata_json")
                .unwrap();
            for name in [
                "native_long_acf",
                "native_short_acf",
                "native_uncoded_acf",
                "broad_spectrum",
                "decoded_spectrum",
                "decoded_lag0",
                "ion_excess",
                "ion_short_raw_ne",
                "ion_short_raw_ne_sigma",
                "ion_raw_ne",
                "ion_raw_ne_sigma",
                "plasma_excess",
            ] {
                let values = if name.ends_with("sigma") {
                    vec![i as f32 + 1.; 2]
                } else {
                    vec![-1. - 2. * i as f32, 2. + 4. * i as f32]
                };
                dataset(&h, name, &[2], &values, "test").unwrap();
            }
        }
        run(input.path(), output.path(), 15., 2).unwrap();
        let files: Vec<_> = fs::read_dir(output.path().join("frames"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        let h = hdf5::File::open(&files[0]).unwrap();
        assert_eq!(
            h.dataset("ion_short_raw_ne")
                .unwrap()
                .read_raw::<f32>()
                .unwrap(),
            vec![-2., 4.]
        );
        let sigma = h
            .dataset("ion_short_raw_ne_sigma")
            .unwrap()
            .read_raw::<f32>()
            .unwrap();
        assert!((sigma[0] - (5f32).sqrt() / 2.).abs() < 1e-6);
        let m = read_meta(&h).unwrap();
        assert_eq!(m.start_unix, 0.);
        assert_eq!(m.end_unix, 15.);
        assert_eq!(
            h.dataset("integration_source_count")
                .unwrap()
                .read_raw::<u32>()
                .unwrap(),
            vec![2]
        );
        drop(h);
        run(input.path(), output.path(), 15., 2).unwrap();
        assert_eq!(
            fs::read_dir(output.path().join("frames")).unwrap().count(),
            1
        );
    }
}
