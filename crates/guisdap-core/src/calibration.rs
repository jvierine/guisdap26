//! Field-aligned, unmagnetized plasma-line calibration.
//! Bohm–Gross thermal approximation and finite-Debye ion-line area.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
const EPS: f64 = 8.8541878128e-12;
const ME: f64 = 9.1093837015e-31;
const E: f64 = 1.602176634e-19;
const KB: f64 = 1.380649e-23;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Estimate {
    pub cold_density_m3: f64,
    pub plasma_density_m3: f64,
    pub thermal_frequency_squared_hz2: f64,
    pub temperature_ratio: f64,
    pub debye_length_m: f64,
    pub k2_debye2: f64,
    pub ion_power_denominator: f64,
    pub fitted_ion_power_denominator: f64,
    pub calibration_multiplier_ratio: f64,
    pub magic_gain_ratio: f64,
}
pub fn frequency_hz(ne: f64, te: f64, radar_hz: f64) -> f64 {
    let k = 4. * std::f64::consts::PI * radar_hz / 299792458.;
    (ne * E * E / (EPS * ME) + 3. * k * k * KB * te / ME).sqrt() / (2. * std::f64::consts::PI)
}
pub fn estimate(
    frequency_hz: f64,
    radar_hz: f64,
    te: f64,
    ti: f64,
    fitted_ne: f64,
) -> Result<Estimate> {
    ensure!(
        [frequency_hz, radar_hz, te, ti, fitted_ne]
            .iter()
            .all(|v| v.is_finite()),
        "nonfinite calibration input"
    );
    ensure!(
        radar_hz > 0. && te > 0. && ti > 0. && fitted_ne > 0.,
        "positive radar frequency, temperatures and fitted density required"
    );
    let k = 4. * std::f64::consts::PI * radar_hz / 299792458.;
    let omega2 = (2. * std::f64::consts::PI * frequency_hz).powi(2);
    let thermal = 3. * k * k * KB * te / ME;
    ensure!(
        omega2 > thermal,
        "thermal term exceeds measured frequency squared: no positive plasma density"
    );
    let ne = (omega2 - thermal) * EPS * ME / (E * E);
    let ld2 = EPS * KB * te / (ne * E * E);
    let q = k * k * ld2;
    let qfit = k * k * EPS * KB * te / (fitted_ne * E * E);
    let ratio = te / ti;
    let denom = (1. + q) * (1. + q + ratio);
    let denom_fit = (1. + qfit) * (1. + qfit + ratio);
    let factor = (ne / denom) / (fitted_ne / denom_fit);
    Ok(Estimate {
        cold_density_m3: omega2 * EPS * ME / (E * E),
        plasma_density_m3: ne,
        thermal_frequency_squared_hz2: thermal / (2. * std::f64::consts::PI).powi(2),
        temperature_ratio: ratio,
        debye_length_m: ld2.sqrt(),
        k2_debye2: q,
        ion_power_denominator: denom,
        fitted_ion_power_denominator: denom_fit,
        calibration_multiplier_ratio: factor,
        magic_gain_ratio: 1. / factor,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_identity() {
        for ne in [1e10, 1e11, 1e12] {
            let f = frequency_hz(ne, 1500., 930e6);
            let a = estimate(-f, 930e6, 1500., 900., ne).unwrap();
            assert!((a.plasma_density_m3 / ne - 1.).abs() < 1e-12);
            assert!((a.calibration_multiplier_ratio - 1.).abs() < 1e-12);
        }
    }
    #[test]
    fn power_scaling_direction_and_debye() {
        let a = estimate(frequency_hz(2e11, 1200., 930e6), 930e6, 1200., 700., 1e11).unwrap();
        assert!(a.calibration_multiplier_ratio > 2.);
        assert!(a.magic_gain_ratio < 0.5);
        assert!(a.ion_power_denominator > 1. + 1200. / 700.);
    }
    #[test]
    fn rejects_nonphysical_and_nonfinite() {
        assert!(estimate(1e5, 930e6, 1000., 1000., 1e11).is_err());
        assert!(estimate(5e6, 930e6, 1000., 0., 1e11).is_err());
        assert!(estimate(f64::NAN, 930e6, 1000., 1000., 1e11).is_err());
    }
}

/// Convert the signed raw-Ne lag proxy to zero-lag equivalent power and a
/// calibration factor, using the actual experiment kernel integrated with the
/// spectrum at the plasma-line density. `response` includes quadrature and ND.
pub fn short_lag_conversion(estimate: &mut Estimate, raw_ne: f64, response: f64) -> Result<f64> {
    ensure!(
        raw_ne.is_finite() && raw_ne > 0. && response.is_finite() && response > 0.,
        "nonpositive or invalid short-lag measurement/response"
    );
    estimate.calibration_multiplier_ratio = 2. * estimate.plasma_density_m3 * response / raw_ne;
    estimate.magic_gain_ratio = 1. / estimate.calibration_multiplier_ratio;
    Ok(raw_ne / (estimate.ion_power_denominator * response))
}
#[cfg(test)]
mod conversion_tests {
    use super::*;
    #[test]
    fn synthetic_lag_recovers_known_gain_and_power() {
        let ne = 1.5e11;
        let mut a = estimate(frequency_hz(ne, 1200., 930e6), 930e6, 1200., 800., 1e11).unwrap();
        for response in [0.03, 0.25, 0.45] {
            let raw = 2. * ne * response / 1.7;
            let zero = short_lag_conversion(&mut a, raw, response).unwrap();
            assert!((a.calibration_multiplier_ratio - 1.7).abs() < 1e-12);
            assert!((zero / (2. * ne / a.ion_power_denominator / 1.7) - 1.).abs() < 1e-12);
        }
        assert!(short_lag_conversion(&mut a, -1., 0.3).is_err());
        assert!(short_lag_conversion(&mut a, 1e11, 0.).is_err());
    }
}

/// Through-origin least squares: predicted raw Ne = gain * measured raw Ne.
/// Conditional standard error from independent time clusters. Points sharing
/// the same one-minute ion fit must share a cluster. Neither x uncertainty nor
/// systematic plasma-model errors are included.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GainFit {
    pub gain: f64,
    pub standard_error: Option<f64>,
    pub points: usize,
    pub time_clusters: usize,
}
pub fn fit_gain(points: &[(f64, f64, u64)]) -> Option<GainFit> {
    if points.is_empty()
        || points
            .iter()
            .any(|(x, y, _)| !x.is_finite() || !y.is_finite() || *x <= 0. || *y <= 0.)
    {
        return None;
    }
    let scale = points.iter().map(|p| p.0).fold(0., f64::max);
    let xx = points.iter().map(|p| (p.0 / scale).powi(2)).sum::<f64>();
    let xy = points
        .iter()
        .map(|p| (p.0 / scale) * (p.1 / scale))
        .sum::<f64>();
    let gain = xy / xx;
    if !gain.is_finite() {
        return None;
    }
    let mut scores = std::collections::BTreeMap::<u64, f64>::new();
    for &(x, y, t) in points {
        let x = x / scale;
        let y = y / scale;
        *scores.entry(t).or_default() += x * (y - gain * x);
    }
    let n = scores.len();
    let standard_error = (n > 1).then(|| {
        ((n as f64 / (n - 1) as f64) * scores.values().map(|s| s * s).sum::<f64>()).sqrt() / xx
    });
    Some(GainFit {
        gain,
        standard_error,
        points: points.len(),
        time_clusters: n,
    })
}
#[cfg(test)]
mod gain_tests {
    use super::*;
    #[test]
    fn known_gain_and_scale_invariance() {
        let p = [(1., 2., 0), (2., 4., 1), (3., 6., 2)];
        let f = fit_gain(&p).unwrap();
        assert_eq!(f.gain, 2.);
        assert_eq!(f.standard_error, Some(0.));
        let q = p.map(|(x, y, t)| (x * 1e12, y * 1e12, t));
        assert_eq!(fit_gain(&q).unwrap().gain, 2.);
    }
    #[test]
    fn correlated_repeated_points_do_not_reduce_uncertainty() {
        let p = [(1., 1., 0), (1., 3., 1)];
        let f = fit_gain(&p).unwrap();
        assert!((f.standard_error.unwrap() - 1.).abs() < 1e-12);
        let repeated = [p[0], p[0], p[1], p[1]];
        assert_eq!(
            fit_gain(&repeated).unwrap().standard_error,
            f.standard_error
        );
        assert!(fit_gain(&[(1., 1., 0), (1., 3., 0)])
            .unwrap()
            .standard_error
            .is_none());
        assert!(fit_gain(&[(0., 1., 0)]).is_none());
    }
}

#[cfg(test)]
mod absolute_gain_test {
    use super::*;
    #[test]
    fn recovered_constant_is_independent_of_input_reference() {
        for reference in [0.7, 1.0, 1.3] {
            let measured = [1e11, 2e11, 3e11].map(|unit| reference * unit);
            let samples: Vec<_> = measured
                .iter()
                .enumerate()
                .map(|(i, &raw)| (raw / reference, 1.4 * (i + 1) as f64 * 1e11, i as u64))
                .collect();
            let fit = fit_gain(&samples).unwrap();
            assert!((fit.gain - 1.4).abs() < 1e-12);
            assert!(fit.standard_error.unwrap() < 1e-12);
        }
    }
}
