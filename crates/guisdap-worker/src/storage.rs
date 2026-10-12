//! Storage precision is independent of the f64 numerical model.
use anyhow::{ensure, Result};
use hdf5::{Dataset, Group};

/// Bulk measurement arrays use IEEE f32, shuffle and portable DEFLATE compression.
/// Reject overflow or nonzero underflow rather than silently losing a measurement.
pub fn array(group: &Group, name: &str, shape: [usize; 2], values: &[f64]) -> Result<Dataset> {
    ensure!(
        shape[0] * shape[1] == values.len(),
        "{name}: shape mismatch"
    );
    let data = values
        .iter()
        .map(|&v| {
            let x = v as f32;
            ensure!(
                !v.is_finite() || (x.is_finite() && (v == 0. || x != 0.)),
                "{name}: value outside float32 range"
            );
            Ok(x)
        })
        .collect::<Result<Vec<_>>>()?;
    // Small arrays stay contiguous: chunk metadata would exceed any savings.
    let ds = if values.len() < 256 {
        group.new_dataset::<f32>().shape(shape).create(name)?
    } else {
        group
            .new_dataset::<f32>()
            .shape(shape)
            .chunk([shape[0].min(256), shape[1]])
            .shuffle()
            .deflate(4)
            .create(name)?
    };
    ds.write_raw(&data)?;
    Ok(ds)
}
/// Keep the full-precision restart record, but compress its repetitive JSON.
pub fn bytes(group: &Group, name: &str, data: &[u8]) -> Result<()> {
    let ds = if data.is_empty() {
        group.new_dataset::<u8>().shape(0).create(name)?
    } else {
        group
            .new_dataset::<u8>()
            .shape(data.len())
            .chunk(data.len().min(65536))
            .deflate(4)
            .create(name)?
    };
    ds.write_raw(data)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_preserves_signed_values_nan_and_precision() -> Result<()> {
        let temp = tempfile::NamedTempFile::new()?;
        let h = hdf5::File::create(temp.path())?;
        let values = [-1.23456789e11, 0., f64::NAN, 1.23456789e13, 1e-6, -2.5];
        let d = array(&h, "values", [3, 2], &values)?;
        assert!(d.dtype()?.is::<f32>());
        assert_eq!(d.shape(), vec![3, 2]);
        let read = d.read_raw::<f64>()?;
        for (a, b) in values.iter().zip(read) {
            if a.is_nan() {
                assert!(b.is_nan())
            } else {
                assert!((a - b).abs() <= a.abs() * 6e-8)
            }
        }
        let bulk: Vec<_> = (0..2048)
            .map(|i| (i as f64 - 1024.) * 1.23456789e8)
            .collect();
        let compressed = array(&h, "bulk", [1024, 2], &bulk)?;
        assert!(compressed.chunk().is_some());
        let recovered = compressed.read_raw::<f64>()?;
        for (a, b) in bulk.iter().zip(recovered) {
            assert!((a - b).abs() <= a.abs() * 6e-8);
        }
        let json = br#"{"time_unix":1667408535.125,"calibration":0.8071913471401251}"#;
        bytes(&h, "metadata", json)?;
        assert_eq!(h.dataset("metadata")?.read_raw::<u8>()?, json);
        assert!(array(&h, "overflow", [1, 1], &[1e100]).is_err());
        assert!(array(&h, "underflow", [1, 1], &[1e-100]).is_err());
        Ok(())
    }
}
