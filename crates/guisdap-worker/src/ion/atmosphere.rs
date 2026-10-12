//! Prescribed atmosphere and IRI DS-1995 molecular composition. No fitted state.
//! DS-1995 is IRI's selectable empirical composition option, not FLIP chemistry.
mod iri_tables;
use anyhow::{ensure, Result};
use chrono::{DateTime, Datelike, Timelike, Utc};
use iri_tables::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtmosphereConfig {
    pub date: String,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub f107_previous_day: f64,
    pub f107_81day: f64,
    pub f107_365day: f64,
    pub ap_daily: f64,
    pub drivers_source: String,
    pub drivers_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prescribed {
    pub model: String,
    pub composition_model: String,
    pub collision_model: String,
    pub unix: f64,
    pub altitude_km: f64,
    pub solar_zenith_deg: f64,
    pub msis_f107_previous_day_observed: f64,
    pub msis_f107_81day_observed: f64,
    pub earth_sun_distance_au: f64,
    pub neutral_temperature_k: f64,
    pub neutral_o_m3: f64,
    pub neutral_n2_m3: f64,
    pub neutral_o2_m3: f64,
    pub collision_s: f64,
    pub molecular_fraction: f64,
    pub iri_no_fraction: f64,
    pub iri_o2_fraction: f64,
    pub iri_cluster_fraction: f64,
    pub below_iri_composition_domain: bool,
}
impl AtmosphereConfig {
    pub fn evaluate(&self, unix: f64, altitude: f64) -> Result<Prescribed> {
        let time = DateTime::<Utc>::from_timestamp(unix as i64, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid atmospheric timestamp"))?;
        ensure!(
            time.format("%Y-%m-%d").to_string() == self.date,
            "no archived atmosphere drivers for {time}"
        );
        ensure!(
            (-90. ..=90.).contains(&self.latitude_deg)
                && (-180. ..=180.).contains(&self.longitude_deg),
            "invalid atmosphere coordinates"
        );
        ensure!(
            [self.f107_previous_day, self.f107_81day, self.f107_365day]
                .iter()
                .all(|x| x.is_finite() && *x > 0.)
                && self.ap_daily >= 0.
                && self.ap_daily.is_finite(),
            "invalid atmosphere drivers"
        );
        let seconds = time.num_seconds_from_midnight() as f64;
        // IRI APF107 is adjusted to 1 AU; MSIS expects observed flux at Earth.
        // Same eccentric-orbit approximation as IRI-2020 IRISUB.
        let am = std::f64::consts::PI * (time.ordinal() as f64 - 3.) / 182.6;
        let eccentricity = 0.01675;
        let radius = 1. - eccentricity * (am.cos() + eccentricity * ((2. * am).cos() - 1.) / 2.);
        let observed_daily = self.f107_previous_day / radius.powi(2);
        let observed_average = self.f107_81day / radius.powi(2);
        let input = rmsis::Input::consistent(
            time.ordinal() as i32,
            seconds,
            altitude,
            self.latitude_deg,
            self.longitude_deg,
            observed_average,
            observed_daily,
            rmsis::MagneticActivity::Daily { ap: self.ap_daily },
        );
        let n = rmsis::Model::new().gtd7::<f64>(&input);
        // GUISDAP IRI prescription: molecular-ion momentum transfer with O,N2,O2.
        // Coefficients m^3/s; SI number densities; common effective rate in two-ion ISR.
        // No arbitrary 1/s floor, and no optimizer dependence.
        let collision = 1.63e-16 * n.atomic_oxygen
            + 2.99e-16 * n.molecular_nitrogen
            + 4.28e-16 * n.molecular_oxygen;
        ensure!(
            collision.is_finite() && collision >= 0.,
            "invalid MSIS collision rate"
        );
        let zenith = solar_zenith(
            time.ordinal(),
            seconds,
            self.latitude_deg,
            self.longitude_deg,
        );
        let month = if self.latitude_deg < 0. {
            (time.month() + 5) % 12 + 1
        } else {
            time.month()
        };
        let fractions = molecular(altitude.max(75.), zenith, month, self.f107_365day);
        let molecular_fraction = (fractions[0] + fractions[1] + fractions[2]).clamp(0., 1.);
        Ok(Prescribed { model:"rmsis NRLMSISE-00 fc707387617885de749dcc516c306a74227cf1b5".into(),
            composition_model:"IRI DS-1995 molecular fraction; remaining fraction represented as O+".into(),
            collision_model:"effective molecular-ion neutral rate: 1.63e-16 nO + 2.99e-16 nN2 + 4.28e-16 nO2 [s^-1]".into(),
            unix, altitude_km:altitude, solar_zenith_deg:zenith,             msis_f107_previous_day_observed: observed_daily,
            msis_f107_81day_observed: observed_average,
            earth_sun_distance_au: radius,
neutral_temperature_k:n.temperature,
            neutral_o_m3:n.atomic_oxygen, neutral_n2_m3:n.molecular_nitrogen, neutral_o2_m3:n.molecular_oxygen,
            collision_s:collision, molecular_fraction, iri_no_fraction:fractions[0], iri_o2_fraction:fractions[1], iri_cluster_fraction:fractions[2],
            below_iri_composition_domain:altitude<75. })
    }
}
fn solar_zenith(day: u32, seconds: f64, lat: f64, lon: f64) -> f64 {
    // Solar longitude/declination and equation of time, fractional-year approximation.
    let g = std::f64::consts::TAU / 365.25 * (day as f64 - 1. + (seconds / 3600. - 12.) / 24.);
    let dec = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2. * g).cos()
        + 0.000907 * (2. * g).sin()
        - 0.002697 * (3. * g).cos()
        + 0.00148 * (3. * g).sin();
    let eq = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2. * g).cos()
            - 0.040849 * (2. * g).sin());
    let ha = (seconds / 240. + lon + eq / 4. - 180.).to_radians();
    let lat = lat.to_radians();
    (lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha.cos())
        .clamp(-1., 1.)
        .acos()
        .to_degrees()
}
fn piece(h: f64, z: f64, j: &[f64], height: &[f64], value: &[f64], slope: &[f64]) -> f64 {
    let knots = [20., 40., 60., 70., 80., 85., 90.];
    let upper = knots.iter().position(|k| z <= *k).unwrap_or(6);
    let at = |col: usize| {
        let n = j[col] as usize;
        let row = (1..n)
            .find(|&i| h < height[col * 13 + i])
            .map(|i| i - 1)
            .unwrap_or(n - 1);
        let k = col * 13 + row;
        value[k] + slope[k] * (h - height[k])
    };
    if upper == 0 || z == knots[upper] {
        at(upper)
    } else {
        let w = (z - knots[upper - 1]) / (knots[upper] - knots[upper - 1]);
        at(upper - 1) + (at(upper) - at(upper - 1)) * w
    }
}
/// NO+, O2+, cluster fractions, reproducing IRI IONCO2 including percent rounding.
/// IRI IONDANI sets molecular ions to zero above 300 km.
pub fn molecular(h: f64, z: f64, month: u32, flux: f64) -> [f64; 3] {
    if h > 300. {
        return [0.; 3];
    }
    let z = z.clamp(20., 90.);
    let eval =
        |j1: &[f64],
         j2: &[f64],
         h1: &[f64],
         h2: &[f64],
         r1: &[f64],
         r2: &[f64],
         s1: &[f64],
         s2: &[f64]| [piece(h, z, j1, h1, r1, s1), piece(h, z, j2, h2, r2, s2)];
    let (low, high) = match month {
        1 | 2 | 11 | 12 => (
            eval(
                &J1MW70, &J2MW70, &H1W70, &H2W70, &R1MW70, &R2MW70, &RK1MW70, &RK2MW70,
            ),
            eval(
                &J1MW140, &J2MW140, &H1W140, &H2W140, &R1MW140, &R2MW140, &RK1MW140, &RK2MW140,
            ),
        ),
        5..=8 => (
            eval(
                &J1MS70, &J2MS70, &H1S70, &H2S70, &R1MS70, &R2MS70, &RK1MS70, &RK2MS70,
            ),
            eval(
                &J1MS140, &J2MS140, &H1S140, &H2S140, &R1MS140, &R2MS140, &RK1MS140, &RK2MS140,
            ),
        ),
        _ => (
            eval(
                &J1MR70, &J2MR70, &H1R70, &H2R70, &R1MR70, &R2MR70, &RK1MR70, &RK2MR70,
            ),
            eval(
                &J1MR140, &J2MR140, &H1R140, &H2R140, &R1MR140, &R2MR140, &RK1MR140, &RK2MR140,
            ),
        ),
    };
    let w = ((flux - 70.) / 70.).clamp(0., 1.);
    let a = low[0] + (high[0] - low[0]) * w;
    let b = low[1] + (high[1] - low[1]) * w;
    let cluster = if h < 100. { (100. - a - b).max(0.) } else { 0. };
    [a.round() / 100., b.round() / 100., cluster.round() / 100.]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iri_matches_original_fortran_reference() {
        let rows: Vec<[f64; 7]> =
            serde_json::from_str(include_str!("atmosphere/iri_reference.json")).unwrap();
        for r in rows {
            let got = molecular(r[0], r[1], r[2] as u32, r[3]);
            for i in 0..3 {
                assert!((got[i] - r[4 + i] / 100.).abs() < 1e-12, "{r:?} vs {got:?}");
            }
        }
    }
}

#[cfg(test)]
mod atmosphere_tests {
    use super::*;
    #[test]
    fn event_msis_profile_is_finite_and_decreases_with_height() {
        let cfg: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../config/ion-fit-msis-iri-full-60s.json"
        ))
        .unwrap();
        let cfg: AtmosphereConfig = serde_json::from_value(cfg["atmosphere"].clone()).unwrap();
        let mut last = f64::INFINITY;
        for h in [52., 75., 100., 120., 150., 200., 300., 500., 690.] {
            let p = cfg.evaluate(1667408700., h).unwrap();
            assert!(
                (p.msis_f107_previous_day_observed
                    - cfg.f107_previous_day / p.earth_sun_distance_au.powi(2))
                .abs()
                    < 1e-12
            );
            assert!(p.collision_s > 0. && p.collision_s < last);
            assert!(p.neutral_temperature_k > 100. && p.neutral_temperature_k < 3000.);
            assert!((0. ..=1.).contains(&p.molecular_fraction));
            let expected =
                1.63e-16 * p.neutral_o_m3 + 2.99e-16 * p.neutral_n2_m3 + 4.28e-16 * p.neutral_o2_m3;
            assert_eq!(p.collision_s, expected);
            println!(
                "altitude={h} collision={} molecular={}",
                p.collision_s, p.molecular_fraction
            );
            last = p.collision_s;
        }
        assert!(cfg.evaluate(0., 100.).is_err());
    }
}
