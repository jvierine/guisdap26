//! Ion-fit inspection. All interaction and plotting run in Rust/WASM.
mod raster;
use super::{el, error, json, text};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};
#[derive(Clone, Deserialize)]
struct Data {
    schema: String,
    uncertainty: String,
    limitations: Vec<String>,
    profiles: Vec<Profile>,
    #[serde(default)]
    processing: Option<Processing>,
}
#[derive(Clone, Deserialize)]
struct Profile {
    start_unix: f64,
    end_unix: f64,
    source_count: usize,
    gates: Vec<Gate>,
    #[serde(default)]
    detail_url: Option<String>,
    #[serde(default)]
    processing_state: String,
    #[serde(default)]
    raw_ne: Vec<Option<[f64; 2]>>,
    #[serde(default)]
    notes: Vec<String>,
}
#[derive(Clone, Deserialize)]
struct Gate {
    first_lag_signal_to_noise: Option<f64>,
    settings: Settings,
    altitude_km: f64,
    lower_km: f64,
    upper_km: f64,
    response_width_max_km: f64,
    oxygen_fraction: f64,
    status: String,
    result: Option<Fit>,
    #[serde(default)]
    lag_us: Vec<f64>,
    #[serde(default)]
    observed: Vec<[f64; 2]>,
    #[serde(default)]
    sigma: Vec<[f64; 2]>,
}
#[derive(Clone, Deserialize)]
struct Settings {
    tie_electron_ion_temperature: bool,
    fit_collision_rate: bool,
}
#[derive(Clone, Deserialize)]
struct Fit {
    plasma: Plasma,
    standard_error: [Option<f64>; 5],
    reduced_chi_square: f64,
    iterations: usize,
    #[serde(default)]
    model_curve: Vec<ModelPoint>,
    #[serde(default)]
    standardized_residual: Vec<[f64; 2]>,
}
#[derive(Clone, Deserialize)]
struct ModelPoint {
    lag_s: f64,
    value: [f64; 2],
    standard_error: Option<[f64; 2]>,
}
#[derive(Clone, Deserialize)]
struct Plasma {
    electron_density_m3: f64,
    ion_temperature_k: f64,
    electron_temperature_k: f64,
    velocity_m_s: f64,
    ion_collision_s: f64,
}
#[derive(Clone, Deserialize)]
struct Processing {
    phase: String,
    threads: usize,
    total: usize,
    completed: usize,
    #[serde(default)]
    failed: usize,
    #[serde(default)]
    insufficient: usize,
    #[serde(default)]
    source_records: usize,
    #[serde(default)]
    updated_unix: i64,
}
#[derive(Clone, Deserialize)]
struct Dataset {
    label: String,
    integration_s: f64,
    data_url: String,
}
#[derive(Deserialize)]
struct Location {
    datasets: Vec<Dataset>,
    #[serde(default)]
    study_status_url: Option<String>,
}
type MapKey = (u64, u64, usize, usize, usize, i64);
struct Viewer {
    data: Data,
    time: usize,
    gate: usize,
    lower: usize,
    upper: usize,
    window_start: f64,
    window_end: f64,
    datasets: Vec<Dataset>,
    resolution: usize,
    details: BTreeMap<usize, Profile>,
    loading: BTreeSet<usize>,
    map_key: std::cell::Cell<Option<MapKey>>,
    statuses: Vec<Option<Processing>>,
    refresh_busy: bool,
    drag: Option<(f64, f64)>,
}
fn selected(v: &Viewer) -> &Profile {
    v.details.get(&v.time).unwrap_or(&v.data.profiles[v.time])
}

fn clock(t: f64) -> String {
    let s = (t.round() as i64).rem_euclid(86400);
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}
fn svg(w: f64, h: f64) -> String {
    format!("<svg viewBox='0 0 {w} {h}' role='img' style='width:100%;display:block;font:12px system-ui;fill:#b6c8d8'>")
}
fn line(s: &mut String, x1: f64, y1: f64, x2: f64, y2: f64, color: &str) {
    s.push_str(&format!(
        "<path d='M{x1:.2},{y1:.2} L{x2:.2},{y2:.2}' fill='none' stroke='{color}'/>"
    ));
}
fn label(s: &mut String, x: f64, y: f64, t: &str, anchor: &str) {
    s.push_str(&format!(
        "<text x='{x:.1}' y='{y:.1}' text-anchor='{anchor}'>{t}</text>"
    ));
}
fn value(p: &Plasma, i: usize) -> f64 {
    match i {
        0 => p.electron_density_m3,
        1 => p.ion_temperature_k,
        2 => p.electron_temperature_k,
        3 => p.velocity_m_s,
        _ => p.ion_collision_s,
    }
}
fn profile(v: &Viewer, param: usize) -> String {
    let mut s = svg(310., 510.);
    let p = selected(v);
    let bottom = p.gates[v.lower].lower_km;
    let top = p.gates[v.upper].upper_km;
    let y = |h: f64| 460. - (h - bottom) / (top - bottom) * 420.;
    let (min, max, ticks): (f64, f64, Vec<(f64, String)>) = match param {
        0 => (
            8.,
            13.,
            (8..=13)
                .map(|a| {
                    (
                        a as f64,
                        ["10⁸", "10⁹", "10¹⁰", "10¹¹", "10¹²", "10¹³"][(a - 8) as usize]
                            .to_string(),
                    )
                })
                .collect(),
        ),
        1 => (
            0.,
            4000.,
            (0..=4)
                .map(|i| ((i * 1000) as f64, (i * 1000).to_string()))
                .collect(),
        ),
        2 => (
            0.,
            8000.,
            (0..=4)
                .map(|i| ((i * 2000) as f64, (i * 2000).to_string()))
                .collect(),
        ),
        _ => (
            -1000.,
            1000.,
            (-2..=2)
                .map(|i| ((i * 500) as f64, (i * 500).to_string()))
                .collect(),
        ),
    };
    let x = |a: f64| 65. + (a - min) / (max - min) * 225.;
    let transform = |a: f64| if param == 0 { a.max(1.).log10() } else { a };
    for h in altitude_ticks(bottom, top) {
        line(&mut s, 65., y(h as f64), 290., y(h as f64), "#233c4c");
        label(&mut s, 57., y(h as f64) + 4., &h.to_string(), "end");
    }
    for (a, t) in ticks {
        line(&mut s, x(a), 40., x(a), 460., "#233c4c");
        label(&mut s, x(a), 480., &t, "middle");
    }
    label(&mut s, 65., 24., "Altitude / km", "start");
    s.push_str("<defs><clipPath id='fit-area'><rect x='64' y='39' width='227' height='423'/></clipPath></defs><g clip-path='url(#fit-area)'>");
    for (i, g) in p.gates.iter().enumerate().take(v.upper + 1).skip(v.lower) {
        if let Some(f) = &g.result {
            let z = value(&f.plasma, param);
            let a = transform(z);
            let color = if g.status == "converged" {
                "#f7be45"
            } else {
                "#b7a4d2"
            };
            if let Some(e) = f.standard_error[param] {
                line(
                    &mut s,
                    x(transform(z - e).max(min)),
                    y(g.altitude_km),
                    x(transform(z + e).min(max)),
                    y(g.altitude_km),
                    color,
                );
            }
            let cx = x(a.clamp(min, max));
            let cy = y(g.altitude_km);
            let fill = if g.status == "converged" {
                color
            } else {
                "#10202a"
            };
            let r = if i == v.gate { 5. } else { 2.8 };
            if a < min || a > max {
                let direction = if a < min { 1. } else { -1. };
                let x1 = cx + direction * 7.;
                s.push_str(&format!("<path d='M{cx:.2},{cy:.2} L{x1:.2},{:.2} L{x1:.2},{:.2} Z' fill='{fill}' stroke='{color}'><title>{z:.3e}; {}</title></path>",cy-4.,cy+4.,g.status));
            } else {
                s.push_str(&format!("<circle cx='{cx:.2}' cy='{cy:.2}' r='{r}' fill='{fill}' stroke='{color}'><title>{z:.3e}; {}</title></circle>",g.status));
            }
        }
    }
    s.push_str("</g></svg>");
    s
}

fn altitude_ticks(bottom: f64, top: f64) -> Vec<i32> {
    let step = if top - bottom <= 150. { 10 } else { 50 };
    let first = (bottom / step as f64).ceil() as i32 * step;
    (first..=top.floor() as i32)
        .step_by(step as usize)
        .collect()
}
fn plasma_color(t: f64) -> String {
    // Samples of the perceptually ordered plasma palette, linearly interpolated.
    let stops = [
        [13., 8., 135.],
        [84., 3., 160.],
        [139., 10., 165.],
        [185., 50., 137.],
        [219., 92., 104.],
        [244., 136., 73.],
        [254., 188., 43.],
        [240., 249., 33.],
    ];
    let p = t.clamp(0., 1.) * (stops.len() - 1) as f64;
    let a = (p.floor() as usize).min(stops.len() - 2);
    let f = p - a as f64;
    let rgb = [0, 1, 2].map(|i| (stops[a][i] * (1. - f) + stops[a + 1][i] * f).round() as u8);
    format!("rgb({},{},{})", rgb[0], rgb[1], rgb[2])
}
fn time_altitude(v: &Viewer, param: usize) -> Result<(), JsValue> {
    let p = &v.data.profiles[v.time];
    let bottom = p.gates[v.lower].lower_km;
    let top = p.gates[v.upper].upper_km;
    let start = v.window_start;
    let end = v.window_end;
    let (min, max) = match param {
        0 | 4 => (8., 13.),
        1 => (0., 4000.),
        2 => (0., 8000.),
        _ => (-1000., 1000.),
    };
    let nx = 1200usize;
    let ny = 240usize;
    let mut pixels = vec![f32::NAN; nx * ny];
    for ix in 0..nx {
        let time = start + (end - start) * (ix as f64 + 0.5) / nx as f64;
        let ti = v.data.profiles.partition_point(|p| p.end_unix <= time);
        let Some(p) = v.data.profiles.get(ti).filter(|p| p.start_unix <= time) else {
            continue;
        };
        for iy in 0..ny {
            let h = bottom + (top - bottom) * (iy as f64 + 0.5) / ny as f64;
            let gi = p.gates.partition_point(|g| g.upper_km <= h);
            let Some(g) = p.gates.get(gi).filter(|g| g.lower_km <= h) else {
                continue;
            };
            let (raw, flag) = if param == 4 {
                let Some(Some(raw)) = p.raw_ne.get(gi) else {
                    continue;
                };
                (raw[0], false)
            } else {
                let Some(f) = &g.result else { continue };
                if g.status == "singular" {
                    continue;
                }
                (value(&f.plasma, param), g.status != "converged")
            };
            if !raw.is_finite() || ((param == 0 || param == 4) && raw <= 0.) {
                continue;
            }
            let z = if param == 0 || param == 4 {
                raw.log10()
            } else {
                raw
            };
            pixels[iy * nx + ix] =
                (((z - min) / (max - min)).clamp(0., 1.) + if flag { 2. } else { 0. }) as f32;
        }
    }
    raster::draw(&format!("fit-raster-{param}"), &pixels, nx, ny)?;
    let mut s = svg(1100., 280.);
    let y = |h: f64| 220. - (h - bottom) / (top - bottom) * 180.;
    for h in altitude_ticks(bottom, top) {
        label(&mut s, 52., y(h as f64) + 4., &h.to_string(), "end");
    }
    label(&mut s, 60., 25., "Altitude / km", "start");
    for i in 0..=4 {
        let t = start + (end - start) * i as f64 / 4.;
        label(
            &mut s,
            60. + 890. * i as f64 / 4.,
            245.,
            &clock(t),
            "middle",
        );
    }
    label(&mut s, 505., 270., "Time / UTC", "middle");
    for i in 0..100 {
        s.push_str(&format!(
            "<rect x='978' y='{:.2}' width='13' height='1.9' fill='{}'/>",
            220. - (i + 1) as f64 * 1.8,
            plasma_color(i as f64 / 99.)
        ));
    }
    for i in 0..=5 {
        let z = min + (max - min) * i as f64 / 5.;
        let t = if param == 0 || param == 4 {
            format!("10^{z:.0}")
        } else {
            format!("{z:.0}")
        };
        label(&mut s, 999., 224. - i as f64 * 36., &t, "start");
    }
    s.push_str("</svg>");
    el(&format!("fit-map-{param}")).set_inner_html(&s);
    Ok(())
}
fn cursor(v: &Viewer, param: usize) {
    let p = &v.data.profiles[v.time];
    let g = &p.gates[v.gate];
    let bottom = p.gates[v.lower].lower_km;
    let top = p.gates[v.upper].upper_km;
    let x = 60.
        + 890. * ((p.start_unix + p.end_unix) / 2. - v.window_start)
            / (v.window_end - v.window_start);
    let y = 220. - (g.altitude_km - bottom) / (top - bottom) * 180.;
    let mut s = svg(1100., 280.);
    if (60. ..=950.).contains(&x) {
        line(&mut s, x, 40., x, 220., "#ffffff");
        line(&mut s, x - 4., y, x + 4., y, "#ffffff");
    }
    s.push_str("</svg>");
    el(&format!("fit-cursor-{param}")).set_inner_html(&s);
}
fn acf(v: &Viewer, component: usize) -> String {
    let g = &selected(v).gates[v.gate];
    let mut s = svg(620., 330.);
    let Some(f) = &g.result else {
        label(&mut s, 50., 80., &g.status, "start");
        s.push_str("</svg>");
        return s;
    };
    let magnitude = g
        .observed
        .iter()
        .zip(&g.sigma)
        .map(|(a, e)| a[component].abs() + e[component])
        .chain(
            f.model_curve
                .iter()
                .map(|p| p.value[component].abs() + p.standard_error.map_or(0., |e| e[component])),
        )
        .fold(1., f64::max);
    let scale = 10f64.powf(magnitude.log10().floor());
    let ymax = (magnitude / scale * 1.1).max(1.);
    let x = |t: f64| 70. + t / 420. * 530.;
    let y = |a: f64| 155. - a / scale / ymax * 115.;
    for i in -2..=2 {
        let a = i as f64 * ymax / 2.;
        line(&mut s, 70., y(a * scale), 600., y(a * scale), "#233c4c");
        label(&mut s, 62., y(a * scale) + 4., &format!("{a:.1}"), "end");
    }
    for t in (0..=400).step_by(100) {
        line(&mut s, x(t as f64), 40., x(t as f64), 270., "#233c4c");
        label(&mut s, x(t as f64), 290., &t.to_string(), "middle");
    }
    label(
        &mut s,
        70.,
        22.,
        &format!("Response-normalized lag / {scale:.0e} m⁻³"),
        "start",
    );
    label(&mut s, 335., 315., "Measured lag / µs", "middle");
    // Draw the band behind measurements, with straight segments only between
    // supplied measured-lag predictions. Never fill an absent uncertainty.
    for segment in f.model_curve.split(|p| p.standard_error.is_none()) {
        if segment.len() < 2 {
            continue;
        }
        let mut path = String::new();
        for (i, p) in segment.iter().enumerate() {
            path.push_str(&format!(
                "{} {:.2},{:.2} ",
                if i == 0 { "M" } else { "L" },
                x(p.lag_s * 1e6),
                y(p.value[component] + p.standard_error.unwrap()[component])
            ));
        }
        for p in segment.iter().rev() {
            path.push_str(&format!(
                "L {:.2},{:.2} ",
                x(p.lag_s * 1e6),
                y(p.value[component] - p.standard_error.unwrap()[component])
            ));
        }
        s.push_str(&format!("<path class='model-band' d='{path} Z' fill='#f7be45' fill-opacity='0.30' stroke='none'/>"));
    }
    for ((&lag, obs), err) in g.lag_us.iter().zip(&g.observed).zip(&g.sigma) {
        let xx = x(lag);
        line(
            &mut s,
            xx,
            y(obs[component] - err[component]),
            xx,
            y(obs[component] + err[component]),
            "#96a9bb",
        );
        s.push_str(&format!(
            "<circle cx='{xx:.2}' cy='{:.2}' r='2.4' fill='#96a9bb'/>",
            y(obs[component])
        ));
    }
    let mut path = String::new();
    for (i, p) in f.model_curve.iter().enumerate() {
        path.push_str(&format!(
            "{} {:.2},{:.2} ",
            if i == 0 { "M" } else { "L" },
            x(p.lag_s * 1e6),
            y(p.value[component])
        ));
    }
    s.push_str(&format!(
        "<path class='model-line' d='{path}' fill='none' stroke='#f7be45' stroke-width='2'/>"
    ));
    s.push_str("</svg>");
    s
}
fn render(v: &Viewer) {
    let p = selected(v);
    let g = &p.gates[v.gate];
    let altitude_slider: HtmlInputElement = el("fit-altitude").dyn_into().unwrap();
    altitude_slider.set_min(&v.lower.to_string());
    altitude_slider.set_max(&v.upper.to_string());
    altitude_slider.set_value(&v.gate.to_string());
    altitude_slider
        .set_attribute(
            "aria-valuetext",
            &format!("{:.0}–{:.0} km", g.lower_km, g.upper_km),
        )
        .unwrap();
    let selector: HtmlSelectElement = el("fit-gate").dyn_into().unwrap();
    selector.set_value(&v.gate.to_string());
    text(
        "fit-altitude-label",
        &format!("Selected altitude: {:.0}–{:.0} km", g.lower_km, g.upper_km),
    );
    text(
        "fit-range-label",
        &format!(
            "Displayed altitude: {:.0}–{:.0} km",
            p.gates[v.lower].lower_km, p.gates[v.upper].upper_km
        ),
    );
    for (id, index, altitude) in [
        ("fit-altitude-min", v.lower, p.gates[v.lower].lower_km),
        ("fit-altitude-max", v.upper, p.gates[v.upper].upper_km),
    ] {
        let slider: HtmlInputElement = el(id).dyn_into().unwrap();
        slider.set_value(&index.to_string());
        slider
            .set_attribute("aria-valuetext", &format!("{altitude:.0} km"))
            .unwrap();
    }
    text(
        "fit-time",
        &format!(
            "{}–{} UTC · {} records",
            clock(p.start_unix),
            clock(p.end_unix),
            p.source_count
        ),
    );
    text(
        "fit-status",
        &format!(
            "{} · {} / {} gates converged",
            p.processing_state,
            p.gates.iter().filter(|g| g.status == "converged").count(),
            p.gates.len()
        ),
    );
    for i in 0..4 {
        el(&format!("fit-profile-{i}")).set_inner_html(&profile(v, i));
    }
    let revision = v.data.processing.as_ref().map_or(0, |p| p.updated_unix);
    let key = (
        v.window_start.to_bits(),
        v.window_end.to_bits(),
        v.lower,
        v.upper,
        v.resolution,
        revision,
    );
    if v.map_key.get() != Some(key) {
        for i in 0..5 {
            if let Err(e) = time_altitude(v, i) {
                text("fit-status", &format!("Plot error: {e:?}"));
            }
        }
        v.map_key.set(Some(key));
    }
    for i in 0..5 {
        cursor(v, i);
    }
    text(
        "fit-window",
        &format!("{}–{} UTC", clock(v.window_start), clock(v.window_end)),
    );
    let first = v
        .data
        .profiles
        .partition_point(|p| p.end_unix <= v.window_start)
        .min(v.data.profiles.len() - 1);
    let last = v
        .data
        .profiles
        .partition_point(|p| p.start_unix < v.window_end)
        .saturating_sub(1);
    let time_slider: HtmlInputElement = el("fit-slider").dyn_into().unwrap();
    time_slider.set_min(&first.to_string());
    time_slider.set_max(&last.to_string());
    time_slider.set_value(&v.time.to_string());
    for (id, t) in [
        ("fit-window-start", v.window_start),
        ("fit-window-end", v.window_end),
    ] {
        el(id)
            .dyn_into::<HtmlInputElement>()
            .unwrap()
            .set_value(&clock(t));
    }
    for i in 0..2 {
        el(&format!("fit-acf-{i}")).set_inner_html(&acf(v, i));
    }
    text(
        "fit-gate-model",
        &format!(
            "Te/Ti: {}. Collision rate: {}. First-lag signal/noise: {:.1}.",
            if g.settings.tie_electron_ion_temperature {
                "fixed to 1"
            } else {
                "fitted"
            },
            if g.settings.fit_collision_rate {
                "fitted effective BGK rate"
            } else {
                "fixed to zero"
            },
            g.first_lag_signal_to_noise.unwrap_or(f64::NAN)
        ),
    );
    let detail = if let Some(f) = &g.result {
        let rms = (f
            .standardized_residual
            .iter()
            .flat_map(|a| a.iter())
            .map(|x| x * x)
            .sum::<f64>()
            / (2 * f.standardized_residual.len()).max(1) as f64)
            .sqrt();
        format!("{} · Ne {:.3e} m⁻³ · Ti {:.0} K · Te {:.0} K · v {:.1} m/s · ν {:.1} s⁻¹ · weighted residual score {:.2} · residual RMS {:.2} · {} iterations",g.status,f.plasma.electron_density_m3,f.plasma.ion_temperature_k,f.plasma.electron_temperature_k,f.plasma.velocity_m_s,f.plasma.ion_collision_s,f.reduced_chi_square,rms,f.iterations)
    } else {
        g.status.clone()
    };
    let detail = if !v.details.contains_key(&v.time) && p.detail_url.is_some() {
        format!("Loading measured/model lags… {}", p.notes.join("; "))
    } else {
        format!("{detail} {}", p.notes.join("; "))
    };
    text("fit-detail", &detail);
    text("fit-support",&format!("Selected altitude bin {:.0}–{:.0} km; maximum equivalent response width {:.2} km (slant). Assumed O⁺ fraction {:.3}. {} response groups; all available decoded ion lags retained.",g.lower_km,g.upper_km,g.response_width_max_km,g.oxygen_fraction,g.lag_us.len()));
}
fn status_view(v: &Viewer) {
    let rows=v.datasets.iter().zip(&v.statuses).map(|(d,p)|{
        if let Some(p)=p {format!("{}: {} — {}/{} intervals; {} failed, {} insufficient-record windows; {} source records; {} worker threads; updated {} UTC",d.label,p.phase,p.completed,p.total,p.failed,p.insufficient,p.source_records,p.threads,clock(p.updated_unix as f64))}
        else{format!("{}: queued / status not yet available",d.label)}
    }).collect::<Vec<_>>().join("\n\n");
    text("processing-fits", &rows);
    if let Some(p) = v.statuses.get(v.resolution).and_then(Option::as_ref) {
        text(
            "fit-progress",
            &format!(
                "{} · {}/{} intervals · {} threads",
                p.phase, p.completed, p.total, p.threads
            ),
        );
    }
}
fn fit_index(data: &Data, time: f64) -> usize {
    data.profiles
        .partition_point(|p| p.end_unix <= time)
        .min(data.profiles.len() - 1)
}
fn set_window(v: &mut Viewer, start: f64, end: f64) {
    let full_start = v.data.profiles[0].start_unix;
    let full_end = v.data.profiles.last().unwrap().end_unix;
    let width = (end - start)
        .max(v.datasets[v.resolution].integration_s)
        .min(full_end - full_start);
    v.window_start = start.clamp(full_start, full_end - width);
    v.window_end = v.window_start + width;
    let selected = (v.data.profiles[v.time].start_unix + v.data.profiles[v.time].end_unix) / 2.;
    v.time = fit_index(
        &v.data,
        selected.clamp(v.window_start + 0.001, v.window_end - 0.001),
    );
}
fn load_detail(state: Rc<RefCell<Viewer>>) {
    let request = {
        let mut v = state.borrow_mut();
        let index = v.time;
        if v.details.contains_key(&index) || v.loading.contains(&index) {
            return;
        }
        let Some(url) = v.data.profiles[index].detail_url.clone() else {
            return;
        };
        let base = v.datasets[v.resolution]
            .data_url
            .rsplit_once('/')
            .map_or("", |(base, _)| base)
            .to_string();
        let resolution = v.resolution;
        v.loading.insert(index);
        (index, resolution, format!("{base}/{url}"))
    };
    spawn_local(async move {
        let result = json::<Profile>(&request.2).await;
        let mut v = state.borrow_mut();
        if v.resolution != request.1 {
            return;
        }
        v.loading.remove(&request.0);
        match result {
            Ok(p) => {
                v.details.insert(request.0, p);
                while v.details.len() > 8 {
                    let furthest = *v
                        .details
                        .keys()
                        .max_by_key(|&&i| i.abs_diff(v.time))
                        .unwrap();
                    v.details.remove(&furthest);
                }
                if v.time == request.0 {
                    render(&v);
                }
            }
            Err(e) => {
                if v.time == request.0 {
                    text(
                        "fit-detail",
                        &format!("Detail load failed; retry by selecting interval: {e:?}"),
                    );
                }
            }
        }
    });
}
async fn change_resolution(state: Rc<RefCell<Viewer>>, resolution: usize) -> Result<(), JsValue> {
    let (url, target) = {
        let v = state.borrow();
        (
            v.datasets[resolution].data_url.clone(),
            v.data.profiles[v.time].start_unix + 0.1,
        )
    };
    el("fit-resolution").set_attribute("disabled", "")?;
    text("fit-progress", "Loading resolution…");
    let loaded = json::<Data>(&url).await;
    el("fit-resolution").remove_attribute("disabled")?;
    let data = loaded?;
    if data.profiles.is_empty() {
        return Err(error("empty resolution"));
    }
    {
        let mut v = state.borrow_mut();
        let start = v.window_start;
        let end = v.window_end;
        v.data = data;
        v.resolution = resolution;
        v.time = fit_index(&v.data, target);
        v.details.clear();
        v.loading.clear();
        v.map_key.set(None);
        v.statuses[resolution] = v.data.processing.clone();
        set_window(&mut v, start, end);
        render(&v);
        status_view(&v);
    }
    load_detail(state);
    Ok(())
}
async fn refresh(state: Rc<RefCell<Viewer>>, study: Option<String>) {
    {
        let mut v = state.borrow_mut();
        if v.refresh_busy {
            return;
        }
        v.refresh_busy = true;
    }
    let datasets = state.borrow().datasets.clone();
    for (i, d) in datasets.iter().enumerate() {
        let status_url = format!(
            "{}/status.json",
            d.data_url.rsplit_once('/').map_or("", |(a, _)| a)
        );
        if let Ok(p) = json::<Processing>(&status_url).await {
            let needs = {
                let v = state.borrow();
                v.resolution == i
                    && v.data
                        .processing
                        .as_ref()
                        .is_none_or(|old| old.completed != p.completed || old.phase != p.phase)
            };
            state.borrow_mut().statuses[i] = Some(p);
            if needs {
                if let Ok(data) = json::<Data>(&d.data_url).await {
                    let mut v = state.borrow_mut();
                    if v.resolution == i && !data.profiles.is_empty() {
                        v.data = data;
                        v.map_key.set(None);
                        render(&v);
                    }
                }
            }
        }
    }
    if let Some(url) = study {
        if let Ok(p) = json::<serde_json::Value>(&url).await {
            text(
                "processing-study",
                &format!("Dataset {} · {} · {} configured worker threads · {} source records\nUpdated {} UTC\n\n{}",p["dataset"].as_str().unwrap_or(""),p["state"].as_str().unwrap_or(""),p["configured_worker_threads"],p["source_records"],clock(p["updated_unix"].as_f64().unwrap_or(0.)),p["stages"].as_array().map(|stages|stages.iter().map(|s|format!("{}: {} — {} / {}",s["name"].as_str().unwrap_or(""),s["state"].as_str().unwrap_or(""),s["completed"],s["total"])).collect::<Vec<_>>().join("\n")).unwrap_or_default()),
            );
        }
    }
    {
        let mut v = state.borrow_mut();
        status_view(&v);
        v.refresh_busy = false;
    }
    load_detail(state);
}
fn seconds(s: &str) -> Option<f64> {
    let parts: Vec<_> = s
        .split(':')
        .map(str::parse::<u32>)
        .collect::<Result<_, _>>()
        .ok()?;
    if parts.len() < 2 || parts[0] > 23 || parts[1] > 59 || parts.get(2).is_some_and(|s| *s > 59) {
        return None;
    }
    Some((parts[0] * 3600 + parts[1] * 60 + parts.get(2).copied().unwrap_or(0)) as f64)
}
#[wasm_bindgen]
pub async fn start_ion() -> Result<(), JsValue> {
    let location: Location = json("ion-config.json").await?;
    if location.datasets.is_empty() {
        return Err(error("no configured resolutions"));
    }
    let data: Data = json(&location.datasets[0].data_url).await?;
    if data.schema != "beata22-ionfit-v2" || data.profiles.is_empty() {
        return Err(error("unsupported or empty ion product"));
    }
    text("fit-uncertainty", &data.uncertainty);
    text("fit-assumptions", &data.limitations.join(". "));
    el("fit-resolution").set_inner_html(
        &location
            .datasets
            .iter()
            .enumerate()
            .map(|(i, d)| format!("<option value='{i}'>{}</option>", d.label))
            .collect::<String>(),
    );
    el("fit-gate").set_inner_html(
        &data.profiles[0]
            .gates
            .iter()
            .enumerate()
            .map(|(i, g)| {
                format!(
                    "<option value='{i}'>{:.0}–{:.0} km</option>",
                    g.lower_km, g.upper_km
                )
            })
            .collect::<String>(),
    );
    let time = fit_index(&data, 1667408700.1);
    let upper = data.profiles[0].gates.len() - 1;
    for id in ["fit-altitude", "fit-altitude-min", "fit-altitude-max"] {
        el(id)
            .dyn_into::<HtmlInputElement>()?
            .set_max(&upper.to_string());
    }
    let gate = data.profiles[0]
        .gates
        .iter()
        .position(|g| g.altitude_km >= 200.)
        .unwrap_or(0);
    let mut statuses = vec![None; location.datasets.len()];
    statuses[0] = data.processing.clone();
    let state = Rc::new(RefCell::new(Viewer {
        window_start: data.profiles[0].start_unix,
        window_end: data.profiles.last().unwrap().end_unix,
        data,
        time,
        gate,
        lower: 0,
        upper,
        datasets: location.datasets,
        resolution: 0,
        details: BTreeMap::new(),
        loading: BTreeSet::new(),
        map_key: std::cell::Cell::new(None),
        statuses,
        refresh_busy: false,
        drag: None,
    }));
    render(&state.borrow());
    status_view(&state.borrow());
    load_detail(state.clone());
    for (id, event) in [
        ("fit-slider", "input"),
        ("fit-gate", "change"),
        ("fit-altitude", "input"),
        ("fit-altitude-min", "input"),
        ("fit-altitude-max", "input"),
        ("fit-window-start", "change"),
        ("fit-window-end", "change"),
        ("fit-zoom-in", "click"),
        ("fit-zoom-out", "click"),
        ("fit-view-all", "click"),
        ("fit-pan-left", "click"),
        ("fit-pan-right", "click"),
    ] {
        let state = state.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            {
                let mut v = state.borrow_mut();
                let last = v.data.profiles[v.time].gates.len() - 1;
                match id {
                    "fit-window-start" | "fit-window-end" => {
                        let day = (v.data.profiles[0].start_unix / 86400.).floor() * 86400.;
                        let a = el("fit-window-start")
                            .dyn_into::<HtmlInputElement>()
                            .unwrap()
                            .value();
                        let b = el("fit-window-end")
                            .dyn_into::<HtmlInputElement>()
                            .unwrap()
                            .value();
                        if let (Some(a), Some(b)) = (seconds(&a), seconds(&b)) {
                            if b > a {
                                set_window(&mut v, day + a, day + b);
                            }
                        }
                    }
                    "fit-view-all" => {
                        let a = v.data.profiles[0].start_unix;
                        let b = v.data.profiles.last().unwrap().end_unix;
                        set_window(&mut v, a, b);
                    }
                    "fit-zoom-in" | "fit-zoom-out" => {
                        let center = (v.data.profiles[v.time].start_unix
                            + v.data.profiles[v.time].end_unix)
                            / 2.;
                        let width = (v.window_end - v.window_start)
                            * if id == "fit-zoom-in" { 0.5 } else { 2. };
                        set_window(&mut v, center - width / 2., center + width / 2.);
                    }
                    "fit-pan-left" | "fit-pan-right" => {
                        let shift = (v.window_end - v.window_start)
                            * if id == "fit-pan-left" { -0.5 } else { 0.5 };
                        let a = v.window_start + shift;
                        let b = v.window_end + shift;
                        set_window(&mut v, a, b);
                    }
                    _ => {
                        let raw = if id == "fit-gate" {
                            el(id).dyn_into::<HtmlSelectElement>().unwrap().value()
                        } else {
                            el(id).dyn_into::<HtmlInputElement>().unwrap().value()
                        };
                        let index = raw.parse::<usize>().unwrap_or(0);
                        match id {
                            "fit-slider" => v.time = index.min(v.data.profiles.len() - 1),
                            "fit-altitude-min" => v.lower = index.min(v.upper),
                            "fit-altitude-max" => v.upper = index.max(v.lower).min(last),
                            _ => v.gate = index.min(last),
                        }
                    }
                }
                v.gate = v.gate.clamp(v.lower, v.upper);
                render(&v);
            }
            load_detail(state.clone());
        });
        el(id).add_event_listener_with_callback(event, cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    {
        let state = state.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let index = el("fit-resolution")
                .dyn_into::<HtmlSelectElement>()
                .unwrap()
                .value()
                .parse::<usize>()
                .unwrap_or(0);
            let state = state.clone();
            spawn_local(async move {
                if let Err(e) = change_resolution(state, index).await {
                    text("fit-progress", &format!("Resolution unavailable: {e:?}"));
                }
            });
        });
        el("fit-resolution")
            .add_event_listener_with_callback("change", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    for (id, processing) in [("show-analysis", false), ("show-processing", true)] {
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let (show, hide) = if processing {
                ("processing-view", "analysis-view")
            } else {
                ("analysis-view", "processing-view")
            };
            el(show).remove_attribute("hidden").unwrap();
            el(hide).set_attribute("hidden", "").unwrap();
        });
        el(id).add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    for param in 0..5 {
        for event in ["mousedown", "mouseup", "mousemove"] {
            let state = state.clone();
            let id = format!("fit-raster-{param}");
            let canvas_id = id.clone();
            let cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(
                move |event_data: web_sys::MouseEvent| {
                    let rect = el(&canvas_id).get_bounding_client_rect();
                    let x =
                        ((event_data.client_x() as f64 - rect.x()) / rect.width()).clamp(0., 1.);
                    let y =
                        ((event_data.client_y() as f64 - rect.y()) / rect.height()).clamp(0., 1.);
                    let mut v = state.borrow_mut();
                    let t = v.window_start + x * (v.window_end - v.window_start);
                    if event == "mousedown" {
                        v.drag = Some((x, t));
                        return;
                    }
                    let ti = fit_index(&v.data, t);
                    let bottom = v.data.profiles[ti].gates[v.lower].lower_km;
                    let top = v.data.profiles[ti].gates[v.upper].upper_km;
                    let h = top - y * (top - bottom);
                    let gi = v.data.profiles[ti]
                        .gates
                        .partition_point(|g| g.upper_km <= h)
                        .min(v.upper);
                    if event == "mousemove" {
                        let p = &v.data.profiles[ti];
                        let g = &p.gates[gi];
                        let val = if param == 4 {
                            p.raw_ne.get(gi).and_then(|a| *a).map(|a| a[0])
                        } else {
                            g.result.as_ref().map(|f| value(&f.plasma, param))
                        };
                        text(
                            "fit-hover",
                            &format!(
                                "{}–{} UTC · {:.0}–{:.0} km · {} · {}",
                                clock(p.start_unix),
                                clock(p.end_unix),
                                g.lower_km,
                                g.upper_km,
                                val.map_or("no value".into(), |n| format!("{n:.4e}")),
                                if param == 4 {
                                    "raw 10 µs lag"
                                } else {
                                    &g.status
                                }
                            ),
                        );
                        return;
                    }
                    if let Some((x0, t0)) = v.drag.take() {
                        if (x - x0).abs() > 0.005 {
                            set_window(&mut v, t.min(t0), t.max(t0));
                        } else {
                            v.time = ti;
                            v.gate = gi;
                        }
                        render(&v);
                        drop(v);
                        load_detail(state.clone());
                    }
                },
            );
            el(&id).add_event_listener_with_callback(event, cb.as_ref().unchecked_ref())?;
            cb.forget();
        }
    }
    let poll_state = state.clone();
    let study = location.study_status_url.clone();
    let poll = Closure::<dyn FnMut()>::new(move || {
        let state = poll_state.clone();
        let study = study.clone();
        spawn_local(refresh(state, study));
    });
    web_sys::window()
        .unwrap()
        .set_interval_with_callback_and_timeout_and_arguments_0(
            poll.as_ref().unchecked_ref(),
            15000,
        )?;
    poll.forget();
    spawn_local(refresh(state, location.study_status_url));
    Ok(())
}
