use anyhow::Result;
fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("MAT model path");
    let m = matfile::MatFile::parse(std::fs::File::open(path)?)?;
    for a in m.arrays() {
        if a.name().starts_with("lpg_")
            || [
                "p_dtau",
                "p_om",
                "p_T0",
                "p_N0",
                "p_om0",
                "p_m0",
                "ch_fradar",
                "vcg_Ap",
                "vcg_pulse",
                "ch_range",
                "p_dt",
            ]
            .contains(&a.name())
        {
            print!("{} {:?}", a.name(), a.size());
            if let matfile::NumericData::Double { real, imag } = a.data() {
                print!(
                    " values={:?} complex={}",
                    &real[..real.len().min(8)],
                    imag.is_some()
                );
            }
            println!();
        }
    }
    Ok(())
}
