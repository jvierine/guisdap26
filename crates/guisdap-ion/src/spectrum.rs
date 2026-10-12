//! Unmagnetized Maxwellian ISR spectrum with number-conserving BGK collisions.
//! Spectral density is per Hz per electron; integrate both frequency signs.
use crate::Complex as C;
use anyhow::{ensure, Result};
use errorfunctions::ComplexErrorFunctions;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
const KB: f64 = 1.380649e-23;
const ME: f64 = 9.1093837139e-31;
const AMU: f64 = 1.66053906892e-27;
const EPS0: f64 = 8.8541878188e-12;
const E: f64 = 1.602176634e-19;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IonSpecies {
    pub mass_amu: f64,
    /// Number fraction. This initial model supports singly charged ions.
    pub fraction: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpectrumModel {
    pub radar_frequency_hz: f64,
    pub ions: Vec<IonSpecies>,
    pub electron_collision_s: f64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PlasmaState {
    pub electron_density_m3: f64,
    pub ion_temperature_k: f64,
    pub electron_temperature_k: f64,
    /// Positive shifts the model spectrum toward positive baseband frequency.
    /// Physical toward/away sign must be established from the receiver convention.
    pub velocity_m_s: f64,
    /// Effective number-conserving BGK rate in s^-1, not cycles per second.
    pub ion_collision_s: f64,
}
impl SpectrumModel {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.radar_frequency_hz.is_finite() && self.radar_frequency_hz > 0.,
            "invalid radar frequency"
        );
        ensure!(
            !self.ions.is_empty()
                && self.ions.iter().all(|i| i.mass_amu.is_finite()
                    && i.mass_amu > 0.
                    && i.fraction.is_finite()
                    && i.fraction >= 0.),
            "invalid composition"
        );
        ensure!(
            (self.ions.iter().map(|i| i.fraction).sum::<f64>() - 1.).abs() < 1e-10,
            "ion fractions must sum to one"
        );
        ensure!(
            self.electron_collision_s.is_finite() && self.electron_collision_s >= 0.,
            "invalid electron collision rate"
        );
        Ok(())
    }
    pub fn wavenumber(&self) -> f64 {
        4. * PI * self.radar_frequency_hz / 299792458.
    }
    pub fn evaluate(&self, state: PlasmaState, frequency_hz: &[f64]) -> Result<Vec<f64>> {
        self.validate()?;
        ensure!(
            state.electron_density_m3.is_finite()
                && state.electron_density_m3 > 0.
                && state.ion_temperature_k.is_finite()
                && state.ion_temperature_k > 0.
                && state.electron_temperature_k.is_finite()
                && state.electron_temperature_k > 0.
                && state.velocity_m_s.is_finite()
                && state.ion_collision_s.is_finite()
                && state.ion_collision_s >= 0.,
            "invalid plasma state"
        );
        ensure!(
            frequency_hz.iter().all(|f| f.is_finite()),
            "invalid frequency grid"
        );
        let k = self.wavenumber();
        let alpha = state.electron_density_m3 * E * E / (EPS0 * KB * k * k);
        let mut output = Vec::with_capacity(frequency_hz.len());
        for &f in frequency_hz {
            let omega = 2. * PI * f - k * state.velocity_m_s;
            let (ae, fe) = response(
                omega,
                k,
                ME,
                state.electron_temperature_k,
                self.electron_collision_s,
            );
            let ce = ae * (alpha / state.electron_temperature_k);
            let mut ci = C::new(0., 0.);
            let mut fi = 0.;
            for ion in &self.ions {
                let (a, free) = response(
                    omega,
                    k,
                    ion.mass_amu * AMU,
                    state.ion_temperature_k,
                    state.ion_collision_s,
                );
                ci += a * (alpha * ion.fraction / state.ion_temperature_k);
                fi += ion.fraction * free;
            }
            let eps = C::new(1., 0.) + ce + ci;
            let s = ((C::new(1., 0.) + ci).norm_sqr() * fe + ce.norm_sqr() * fi) / eps.norm_sqr();
            ensure!(s.is_finite() && s >= 0., "invalid model spectrum");
            output.push(s);
        }
        Ok(output)
    }
}
fn response(omega: f64, k: f64, mass: f64, temperature: f64, collision: f64) -> (C, f64) {
    let a = k * (2. * KB * temperature / mass).sqrt();
    let g0 = C::new(omega / a, collision / a).w() * (PI.sqrt() / a);
    let g = g0 / (C::new(1., 0.) - collision * g0);
    (C::new(1., 0.) + C::new(0., omega) * g, 2. * g.re)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maxwellian_response_matches_independent_time_quadrature() {
        // Independent Simpson integration of the free-particle correlation.
        for nu in [0., 300., 5000.] {
            let k = 39.;
            let t = 900.;
            let mass = 30.5 * AMU;
            let a = k * (2. * KB * t / mass).sqrt();
            for omega in [0., 2000., -8000., 16000.] {
                let n = 10000;
                let dt = 14. / a / n as f64;
                let mut g = C::new(0., 0.);
                for i in 0..=n {
                    let x = i as f64 * dt;
                    let weight = if i == 0 || i == n {
                        1.
                    } else if i % 2 == 0 {
                        2.
                    } else {
                        4.
                    };
                    g += C::from_polar(weight * (-a * a * x * x / 4. - nu * x).exp(), omega * x);
                }
                g *= dt / 3.;
                g /= C::new(1., 0.) - nu * g;
                let (chi, s) = response(omega, k, mass, t, nu);
                assert!((s - 2. * g.re).abs() < 1e-12);
                assert!((chi - (C::new(1., 0.) + C::new(0., omega) * g)).norm() < 1e-8);
            }
        }
    }
    #[test]
    fn spectrum_symmetry_drift_and_composition() {
        let m = SpectrumModel {
            radar_frequency_hz: 930e6,
            ions: vec![
                IonSpecies {
                    mass_amu: 16.,
                    fraction: 0.7,
                },
                IonSpecies {
                    mass_amu: 30.5,
                    fraction: 0.3,
                },
            ],
            electron_collision_s: 0.,
        };
        let p = PlasmaState {
            electron_density_m3: 3e11,
            ion_temperature_k: 900.,
            electron_temperature_k: 1800.,
            velocity_m_s: 0.,
            ion_collision_s: 100.,
        };
        let f = vec![-2000., 0., 2000.];
        let a = m.evaluate(p, &f).unwrap();
        assert!((a[0] - a[2]).abs() < 1e-15);
        let mut moving = p;
        moving.velocity_m_s = 123.;
        let shift = m.wavenumber() * 123. / (2. * PI);
        let b = m
            .evaluate(moving, &f.iter().map(|x| x + shift).collect::<Vec<_>>())
            .unwrap();
        for (a, b) in a.iter().zip(b) {
            assert!((a - b).abs() < 1e-15);
        }
        let mut reordered = m.clone();
        reordered.ions.reverse();
        assert_eq!(
            m.evaluate(p, &f).unwrap(),
            reordered.evaluate(p, &f).unwrap()
        );
    }
}

#[cfg(test)]
mod normalization_tests {
    use super::*;
    #[test]
    fn ion_line_area_matches_equilibrium_debye_limit() {
        let m = SpectrumModel {
            radar_frequency_hz: 930e6,
            ions: vec![IonSpecies {
                mass_amu: 30.5,
                fraction: 1.,
            }],
            electron_collision_s: 0.,
        };
        let p = PlasmaState {
            electron_density_m3: 1e12,
            ion_temperature_k: 1000.,
            electron_temperature_k: 1000.,
            velocity_m_s: 0.,
            ion_collision_s: 0.,
        };
        let f: Vec<_> = (-8000..=8000).map(|i| i as f64 * 10.).collect();
        let s = m.evaluate(p, &f).unwrap();
        let area = s
            .iter()
            .enumerate()
            .map(|(i, value)| value * if i == 0 || i == s.len() - 1 { 5. } else { 10. })
            .sum::<f64>();
        let a = p.electron_density_m3 * E * E
            / (EPS0 * KB * p.electron_temperature_k * m.wavenumber().powi(2));
        let expected = a * a / ((1. + a) * (1. + 2. * a));
        // Finite electron mass and the included electron continuum make the
        // finite ion-band integral differ slightly from the zero-mass limit.
        assert!(
            (area / expected - 1.).abs() < 0.015,
            "area {area}, equilibrium {expected}"
        );
    }
}
