//! Minimal HDF5 reader: cargo run -p guisdap-worker --example read_ion_product -- interval.h5
fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Supply an ion-profile HDF5 path"))?;
    let h = hdf5::File::open(path)?;
    // HDF5 converts old float64 and new float32 files to the requested reader type.
    let time = h.dataset("profile/time_bounds_unix_s")?.read_raw::<f64>()?;
    let altitude = h.dataset("profile/altitude_km")?.read_raw::<f64>()?;
    let parameters = h
        .dataset("profile/parameters_ne_ti_te_v_nu")?
        .read_2d::<f64>()?;
    let errors = h.dataset("profile/standard_error")?.read_2d::<f64>()?;
    println!("UTC Unix seconds: {} to {}", time[0], time[1]);
    println!("altitude_km,ne_m-3,ne_sigma_m-3,ti_K,te_K,velocity_m_s,collision_s-1");
    for (i, h) in altitude.iter().enumerate() {
        println!(
            "{h},{},{},{},{},{},{}",
            parameters[[i, 0]],
            errors[[i, 0]],
            parameters[[i, 1]],
            parameters[[i, 2]],
            parameters[[i, 3]],
            parameters[[i, 4]]
        );
    }
    Ok(())
}
