//! Ion-fit inspection. All interaction and plotting run in Rust/WASM.
use super::{el, error, json, text};
use serde::Deserialize;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{HtmlInputElement, HtmlSelectElement};
#[derive(Deserialize)]
struct Data {
    schema: String,
    uncertainty: String,
    limitations: Vec<String>,
    profiles: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    start_unix: f64,
    end_unix: f64,
    source_count: usize,
    gates: Vec<Gate>,
}
#[derive(Deserialize)]
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
    lag_us: Vec<f64>,
    observed: Vec<[f64; 2]>,
    sigma: Vec<[f64; 2]>,
}
#[derive(Deserialize)]
struct Settings {
    tie_electron_ion_temperature: bool,
    fit_collision_rate: bool,
}
#[derive(Deserialize)]
struct Fit {
    plasma: Plasma,
    standard_error: [Option<f64>; 5],
    reduced_chi_square: f64,
    iterations: usize,
    #[serde(default)]
    model_curve: Vec<ModelPoint>,
    standardized_residual: Vec<[f64; 2]>,
}
#[derive(Deserialize)]
struct ModelPoint {
    lag_s: f64,
    value: [f64; 2],
    standard_error: Option<[f64; 2]>,
}
#[derive(Deserialize)]
struct Plasma {
    electron_density_m3: f64,
    ion_temperature_k: f64,
    electron_temperature_k: f64,
    velocity_m_s: f64,
    ion_collision_s: f64,
}
struct Viewer {
    data: Data,
    time: usize,
    gate: usize,
    lower: usize,
    upper: usize,
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
    let p = &v.data.profiles[v.time];
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
fn time_altitude(v: &Viewer, param: usize) -> String {
    let mut s = svg(620., 365.);
    let selected = &v.data.profiles[v.time];
    let bottom = selected.gates[v.lower].lower_km;
    let top = selected.gates[v.upper].upper_km;
    let start = v.data.profiles[0].start_unix;
    let end = v.data.profiles.last().unwrap().end_unix;
    let x = |t: f64| 60. + (t - start) / (end - start) * 470.;
    let y = |h: f64| 290. - (h - bottom) / (top - bottom) * 250.;
    let (min, max, ticks): (f64, f64, Vec<(f64, String)>) = match param {
        0 => (
            8.,
            13.,
            (8..=13).map(|i| (i as f64, format!("10^{i}"))).collect(),
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
    s.push_str(&format!("<defs><pattern id='fit-hatch-{param}' patternUnits='userSpaceOnUse' width='6' height='6'><path d='M0,6 L6,0' stroke='#e0e0e0' stroke-opacity='0.55' stroke-width='1'/></pattern></defs>"));
    s.push_str("<rect x='60' y='40' width='470' height='250' fill='#10202a'/>");
    for (ti, p) in v.data.profiles.iter().enumerate() {
        for (gi, g) in p.gates.iter().enumerate().take(v.upper + 1).skip(v.lower) {
            let Some(f) = &g.result else { continue };
            if g.status == "singular" {
                continue;
            }
            let raw = value(&f.plasma, param);
            if !raw.is_finite() || (param == 0 && raw <= 0.) {
                continue;
            }
            let z = if param == 0 { raw.log10() } else { raw };
            let color = plasma_color((z - min) / (max - min));
            let xx = x(p.start_unix);
            let yy = y(g.upper_km);
            let width = x(p.end_unix) - xx;
            let height = y(g.lower_km) - yy;
            let title = format!(
                "{}–{} UTC; {:.0}–{:.0} km; {:.3e}; {}",
                clock(p.start_unix),
                clock(p.end_unix),
                g.lower_km,
                g.upper_km,
                raw,
                g.status
            );
            s.push_str(&format!("<rect class='fit-cell' data-time='{ti}' data-gate='{gi}' x='{xx:.2}' y='{yy:.2}' width='{width:.2}' height='{height:.2}' fill='{color}'><title>{title}</title></rect>"));
            if g.status != "converged" {
                s.push_str(&format!("<rect x='{xx:.2}' y='{yy:.2}' width='{width:.2}' height='{height:.2}' fill='url(#fit-hatch-{param})' pointer-events='none'/>"));
            }
        }
    }
    let gate = &selected.gates[v.gate];
    s.push_str(&format!("<rect x='{:.2}' y='{:.2}' width='{:.2}' height='{:.2}' fill='none' stroke='white' stroke-width='1.5' pointer-events='none'/>",x(selected.start_unix),y(gate.upper_km),x(selected.end_unix)-x(selected.start_unix),y(gate.lower_km)-y(gate.upper_km)));
    for h in altitude_ticks(bottom, top) {
        label(&mut s, 52., y(h as f64) + 4., &h.to_string(), "end");
    }
    for i in 0..=3 {
        let t = start + (end - start) * i as f64 / 3.;
        label(&mut s, x(t), 310., &clock(t)[..5], "middle");
    }
    label(&mut s, 60., 24., "Altitude / km", "start");
    label(&mut s, 295., 340., "Time / UTC", "middle");
    for i in 0..100 {
        s.push_str(&format!(
            "<rect x='545' y='{:.2}' width='12' height='2.6' fill='{}'/>",
            290. - (i + 1) as f64 * 2.5,
            plasma_color(i as f64 / 99.)
        ));
    }
    for (z, t) in ticks {
        label(
            &mut s,
            562.,
            290. - (z - min) / (max - min) * 250. + 4.,
            &t,
            "start",
        );
    }
    s.push_str("</svg>");
    s
}

fn acf(v: &Viewer, component: usize) -> String {
    let g = &v.data.profiles[v.time].gates[v.gate];
    let mut s = svg(620., 330.);
    let Some(f) = &g.result else {
        label(
            &mut s,
            50.,
            80.,
            "No identifiable fit in this gate",
            "start",
        );
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
    let p = &v.data.profiles[v.time];
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
            "{} / {} gates converged",
            p.gates.iter().filter(|g| g.status == "converged").count(),
            p.gates.len()
        ),
    );
    for i in 0..4 {
        el(&format!("fit-profile-{i}")).set_inner_html(&profile(v, i));
        el(&format!("fit-map-{i}")).set_inner_html(&time_altitude(v, i));
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
    text("fit-detail", &detail);
    text("fit-support",&format!("Selected altitude bin {:.0}–{:.0} km; maximum equivalent response width {:.2} km (slant). Assumed O⁺ fraction {:.3}. {} response groups; all available decoded ion lags retained.",g.lower_km,g.upper_km,g.response_width_max_km,g.oxygen_fraction,g.lag_us.len()));
}
#[wasm_bindgen]
pub async fn start_ion() -> Result<(), JsValue> {
    #[derive(Deserialize)]
    struct Location {
        data_url: String,
    }
    let location: Location = json("ion-config.json").await?;
    let data: Data = json(&location.data_url).await?;
    if data.schema != "beata22-ionfit-v1"
        || data.profiles.is_empty()
        || data.profiles[0].gates.is_empty()
    {
        return Err(error("unsupported or empty ion-fit product"));
    }
    text("fit-uncertainty", &data.uncertainty);
    let limitation = data.limitations.join(". ");
    text("fit-assumptions", &limitation);
    let options = data.profiles[0]
        .gates
        .iter()
        .enumerate()
        .map(|(i, g)| {
            format!(
                "<option value='{i}'>{:.0}–{:.0} km</option>",
                g.lower_km, g.upper_km
            )
        })
        .collect::<String>();
    el("fit-gate").set_inner_html(&options);
    let slider: HtmlInputElement = el("fit-slider").dyn_into()?;
    slider.set_max(&(data.profiles.len() - 1).to_string());
    let gate = data.profiles[0]
        .gates
        .iter()
        .position(|g| g.altitude_km >= 200.)
        .unwrap_or(0);
    // Keep the initial event time near 17:05 regardless of integration cadence.
    let target = data.profiles[0].start_unix + 300.;
    let time = data
        .profiles
        .iter()
        .position(|p| p.end_unix > target)
        .unwrap_or(data.profiles.len() - 1);
    slider.set_value(&time.to_string());
    let selector: HtmlSelectElement = el("fit-gate").dyn_into()?;
    selector.set_value(&gate.to_string());
    let upper = data.profiles[0].gates.len() - 1;
    for id in ["fit-altitude", "fit-altitude-min", "fit-altitude-max"] {
        let slider: HtmlInputElement = el(id).dyn_into()?;
        slider.set_max(&upper.to_string());
    }
    let state = Rc::new(RefCell::new(Viewer {
        data,
        time,
        gate,
        lower: 0,
        upper,
    }));
    render(&state.borrow());
    for (id, event) in [
        ("fit-slider", "input"),
        ("fit-gate", "change"),
        ("fit-altitude", "input"),
        ("fit-altitude-min", "input"),
        ("fit-altitude-max", "input"),
    ] {
        let state = state.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            let mut s = state.borrow_mut();
            let raw = if id == "fit-gate" {
                el(id).dyn_into::<HtmlSelectElement>().unwrap().value()
            } else {
                el(id).dyn_into::<HtmlInputElement>().unwrap().value()
            };
            let index = raw.parse::<usize>().unwrap_or(0);
            let last = s.data.profiles[s.time].gates.len() - 1;
            match id {
                "fit-slider" => s.time = index.min(s.data.profiles.len() - 1),
                "fit-altitude-min" => s.lower = index.min(s.upper),
                "fit-altitude-max" => s.upper = index.max(s.lower).min(last),
                _ => s.gate = index.min(last),
            }
            s.gate = s.gate.clamp(s.lower, s.upper);
            render(&s);
        });
        el(id).add_event_listener_with_callback(event, cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    Ok(())
}
