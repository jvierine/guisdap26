use super::*;
pub fn write(path: &Path, p: &Profile, key: &str, frequency: &[f64]) -> Result<()> {
    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    {
        let h = hdf5::File::create(tmp.path())?;
        h.new_dataset_builder()
            .with_data(serde_json::to_vec(p)?.as_slice())
            .create("profile_json")?;
        h.new_dataset_builder()
            .with_data(key.as_bytes())
            .create("cache_key")?;
        h.new_dataset_builder()
            .with_data(frequency)
            .create("model_frequency_hz")?;
        let gr = h.create_group("profile")?;
        gr.new_dataset_builder()
            .with_data(&[p.start_unix, p.end_unix])
            .create("time_bounds_unix_s")?;
        gr.new_dataset_builder()
            .with_data(&p.gates.iter().map(|g| g.altitude_km).collect::<Vec<_>>())
            .create("altitude_km")?;
        let values: Vec<_> = p
            .gates
            .iter()
            .flat_map(|g| {
                g.result
                    .as_ref()
                    .map(|r| {
                        [
                            r.plasma.electron_density_m3,
                            r.plasma.ion_temperature_k,
                            r.plasma.electron_temperature_k,
                            r.plasma.velocity_m_s,
                            r.plasma.ion_collision_s,
                        ]
                    })
                    .unwrap_or([f64::NAN; 5])
            })
            .collect();
        let errors: Vec<_> = p
            .gates
            .iter()
            .flat_map(|g| {
                g.result
                    .as_ref()
                    .map(|r| r.standard_error.map(|e| e.unwrap_or(f64::NAN)))
                    .unwrap_or([f64::NAN; 5])
            })
            .collect();
        for (name, data) in [
            ("parameters_ne_ti_te_v_nu", values),
            ("standard_error", errors),
        ] {
            let d = gr
                .new_dataset::<f64>()
                .shape([p.gates.len(), 5])
                .create(name)?;
            d.write_raw(&data)?;
            d.new_attr_builder()
                .with_data("m^-3,K,K,m s^-1,s^-1")
                .create("column_units")?;
        }
        for (j, g) in p.gates.iter().enumerate() {
            let gg = gr.create_group(&format!("gate_{j:03}"))?;
            gg.new_dataset_builder()
                .with_data(&g.lag_us)
                .create("lag_us")?;
            gg.new_dataset_builder()
                .with_data(&[g.lower_km, g.upper_km])
                .create("altitude_bin_km")?;
            gg.new_dataset_builder()
                .with_data(g.status.as_bytes())
                .create("status_utf8")?;
            let mut arrays = vec![("observed", g.observed.clone()), ("sigma", g.sigma.clone())];
            if let Some(r) = &g.result {
                arrays.push(("predicted", r.predicted.clone()));
                arrays.push(("standardized_residual", r.standardized_residual.clone()));
                let curve: Vec<f64> = r
                    .model_curve
                    .iter()
                    .flat_map(|p| {
                        let e = p.standard_error.unwrap_or([f64::NAN; 2]);
                        [p.lag_s, p.value[0], p.value[1], e[0], e[1]]
                    })
                    .collect();
                let ds = gg
                    .new_dataset::<f64>()
                    .shape([r.model_curve.len(), 5])
                    .create("model_curve")?;
                ds.write_raw(&curve)?;
                ds.new_attr_builder()
                    .with_data(
                        "lag_s,mean_real,mean_imag,sigma_real,sigma_imag; s,m^-3,m^-3,m^-3,m^-3",
                    )
                    .create("columns")?;
                let n = r.free_parameter_indices.len();
                if r.covariance.len() == n * n {
                    let ds = gg
                        .new_dataset::<f64>()
                        .shape([n, n])
                        .create("parameter_covariance")?;
                    ds.write_raw(&r.covariance)?;
                }
                gg.new_dataset_builder()
                    .with_data(
                        &r.free_parameter_indices
                            .iter()
                            .map(|i| *i as u32)
                            .collect::<Vec<_>>(),
                    )
                    .create("free_parameter_indices")?;
            }
            for (name, values) in arrays {
                if values.is_empty() {
                    continue;
                }
                let flat: Vec<_> = values.iter().flatten().copied().collect();
                let ds = gg
                    .new_dataset::<f64>()
                    .shape([values.len(), 2])
                    .create(name)?;
                ds.write_raw(&flat)?;
                ds.new_attr_builder()
                    .with_data(if name == "standardized_residual" {
                        "1"
                    } else {
                        "m^-3; real,imag; response-normalized scattering strength"
                    })
                    .create("units")?;
            }
            let mut offsets = vec![0u32];
            let mut addresses = vec![];
            for row in &g.addresses {
                addresses.extend(row.iter().map(|a| *a as u32));
                offsets.push(addresses.len() as u32);
            }
            gg.new_dataset_builder()
                .with_data(&offsets)
                .create("source_address_offsets")?;
            gg.new_dataset_builder()
                .with_data(&addresses)
                .create("source_addresses_zero_based")?;
        }

        let raw: Vec<_> = p
            .raw_ne
            .iter()
            .flat_map(|v| v.unwrap_or([f64::NAN; 2]))
            .collect();
        let ds = gr
            .new_dataset::<f64>()
            .shape([p.raw_ne.len(), 2])
            .create("raw_ne_and_sigma")?;
        ds.write_raw(&raw)?;
        ds.new_attr_builder()
            .with_data("m^-3; signed 10 us lag equivalent; mean,sigma")
            .create("units")?;
        h.flush()?;
    }
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
