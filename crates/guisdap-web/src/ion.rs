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
    predicted: Vec<[f64; 2]>,
    standardized_residual: Vec<[f64; 2]>,
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
    let y = |h: f64| 460. - (h - 40.) / 660. * 420.;
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
    for h in (50..=700).step_by(50) {
        line(&mut s, 65., y(h as f64), 290., y(h as f64), "#233c4c");
        label(&mut s, 57., y(h as f64) + 4., &h.to_string(), "end");
    }
    for (a, t) in ticks {
        line(&mut s, x(a), 40., x(a), 460., "#233c4c");
        label(&mut s, x(a), 480., &t, "middle");
    }
    label(&mut s, 65., 24., "Altitude / km", "start");
    s.push_str("<defs><clipPath id='fit-area'><rect x='64' y='39' width='227' height='423'/></clipPath></defs><g clip-path='url(#fit-area)'>");
    for (i, g) in p.gates.iter().enumerate() {
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
        .chain(f.predicted.iter().map(|a| a[component].abs()))
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
    for (((&lag, obs), err), pred) in g
        .lag_us
        .iter()
        .zip(&g.observed)
        .zip(&g.sigma)
        .zip(&f.predicted)
    {
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
        s.push_str(&format!(
            "<circle cx='{xx:.2}' cy='{:.2}' r='2.5' fill='#f7be45'/>",
            y(pred[component])
        ));
    }
    s.push_str("</svg>");
    s
}
fn render(v: &Viewer) {
    let p = &v.data.profiles[v.time];
    let g = &p.gates[v.gate];
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
    slider.set_value("5");
    let gate = data.profiles[0]
        .gates
        .iter()
        .position(|g| g.altitude_km >= 200.)
        .unwrap_or(0);
    let time = 5.min(data.profiles.len() - 1);
    let selector: HtmlSelectElement = el("fit-gate").dyn_into()?;
    selector.set_value(&gate.to_string());
    let state = Rc::new(RefCell::new(Viewer { data, time, gate }));
    render(&state.borrow());
    for (id, event, is_time) in [("fit-slider", "input", true), ("fit-gate", "change", false)] {
        let state = state.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            let mut s = state.borrow_mut();
            if is_time {
                let input: HtmlInputElement = el("fit-slider").dyn_into().unwrap();
                s.time = input
                    .value()
                    .parse::<usize>()
                    .unwrap_or(0)
                    .min(s.data.profiles.len() - 1);
            } else {
                let select: HtmlSelectElement = el("fit-gate").dyn_into().unwrap();
                s.gate = select
                    .value()
                    .parse::<usize>()
                    .unwrap_or(0)
                    .min(s.data.profiles[s.time].gates.len() - 1);
            }
            render(&s);
        });
        el(id).add_event_listener_with_callback(event, cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    Ok(())
}
