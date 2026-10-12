//! Static-hosted browser application. Data selection, scaling, UI and WebGL live in Rust.
#![cfg(target_arch = "wasm32")]
mod calibration;
mod ion;
use guisdap_core::{merge_bands, native_spectra};
use serde::Deserialize;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    HtmlCanvasElement, HtmlInputElement, HtmlSelectElement, WebGl2RenderingContext as Gl,
};
#[derive(Deserialize)]
struct Config {
    data_root: String,
    #[serde(default)]
    datasets: Vec<PlasmaDataset>,
}
#[derive(Deserialize)]
struct PlasmaDataset {
    label: String,
    data_root: String,
}
#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    source_count: usize,
    id: String,
    start_unix: f64,
    end_unix: f64,
    elevation_deg: f64,
    azimuth_deg: f64,
    transmitter_w: f64,
    centers_hz: [f64; 2],
}
#[derive(Deserialize)]
struct Manifest {
    schema: String,
    frames: Vec<Frame>,
    ion_range_km: Vec<f64>,
    decoded_range_km: Vec<f64>,
    #[serde(default)]
    overview_root: String,
}
struct App {
    calibration: Option<calibration::Cal>,
    profile_altitude: [f64; 2],
    profile_power: [f64; 2],
    broad_frequency: [f64; 2],
    root: String,
    m: Manifest,
    broad: Vec<f32>,
    raw: Vec<f32>,
    power: Vec<f32>,
    power_sigma: Vec<f32>,
    decoded: Vec<f32>,
    selected: usize,
    view_start: f64,
    view_end: f64,
    decoded_selected: Option<usize>,
    raw_view: bool,
    percentile: f64,
    busy: bool,
    request: u64,
    smoothing: usize,
    magnitude: bool,
    weighted: bool,
    limit: Option<f32>,
    native: Vec<f32>,
    cache: std::collections::VecDeque<(usize, Vec<f32>)>,
}
type State = Rc<RefCell<App>>;
fn doc() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}
fn el(id: &str) -> web_sys::Element {
    doc().get_element_by_id(id).unwrap()
}
fn text(id: &str, s: &str) {
    el(id).set_text_content(Some(s));
}
fn error(s: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&s.to_string())
}
fn report(e: JsValue) {
    text(
        "status",
        &format!(
            "Load failed: {}",
            e.as_string().unwrap_or_else(|| format!("{e:?}"))
        ),
    );
    web_sys::console::error_1(&e);
}
async fn response(url: &str) -> Result<web_sys::Response, JsValue> {
    let request = web_sys::RequestInit::new();
    if url.ends_with("config.json") || url.ends_with("index.json") || url.ends_with("status.json") {
        request.set_cache(web_sys::RequestCache::NoCache);
    }
    let r: web_sys::Response = JsFuture::from(
        web_sys::window()
            .unwrap()
            .fetch_with_str_and_init(url, &request),
    )
    .await?
    .dyn_into()?;
    if !r.ok() {
        return Err(error(format!("HTTP {}: {url}", r.status())));
    }
    Ok(r)
}
async fn json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, JsValue> {
    let r = response(url).await?;
    let s = JsFuture::from(r.text()?)
        .await?
        .as_string()
        .ok_or_else(|| error("expected text"))?;
    serde_json::from_str(&s).map_err(error)
}
async fn floats(url: &str) -> Result<Vec<f32>, JsValue> {
    let r = response(url).await?;
    let b = JsFuture::from(r.array_buffer()?).await?;
    let bytes = js_sys::Uint8Array::new(&b).to_vec();
    if !bytes.len().is_multiple_of(4) {
        return Err(error("truncated float product"));
    }
    let v: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    if !v.iter().all(|v| v.is_finite()) {
        return Err(error("non-finite exported product"));
    }
    Ok(v)
}
fn utc(t: f64) -> String {
    let s: String = js_sys::Date::new(&JsValue::from_f64(t * 1000.))
        .to_iso_string()
        .into();
    s[11..19].into()
}
fn altitude(r: f64, e: f64) -> f64 {
    let earth = 6370.03;
    (earth * earth + r * r + 2. * earth * r * e.to_radians().sin()).sqrt() - 6370.
}
fn percentile(v: &[f32], p: f64) -> f32 {
    let mut a: Vec<f32> = v
        .iter()
        .map(|x| x.abs())
        .filter(|x| x.is_finite())
        .collect();
    a.sort_by(f32::total_cmp);
    a.get(((a.len().saturating_sub(1)) as f64 * p) as usize)
        .copied()
        .unwrap_or(1.)
        .max(1e-12)
}
fn shader(gl: &Gl, kind: u32, src: &str) -> Result<web_sys::WebGlShader, JsValue> {
    let s = gl
        .create_shader(kind)
        .ok_or_else(|| error("WebGL shader allocation"))?;
    gl.shader_source(&s, src);
    gl.compile_shader(&s);
    if gl.get_shader_parameter(&s, Gl::COMPILE_STATUS).as_bool() != Some(true) {
        return Err(error(gl.get_shader_info_log(&s).unwrap_or_default()));
    }
    Ok(s)
}
fn draw(
    id: &str,
    v: &[f32],
    width: usize,
    height: usize,
    p: f64,
    cursor: Option<f32>,
    scale: (Option<f32>, bool),
) -> Result<(), JsValue> {
    if v.len() != width * height || width == 0 || height == 0 {
        return Err(error(format!("invalid texture {id}")));
    }
    let (fixed_limit, positive) = scale;
    let canvas: HtmlCanvasElement = el(id).dyn_into()?;
    let gl: Gl = canvas
        .get_context("webgl2")?
        .ok_or_else(|| error("WebGL2 is required"))?
        .dyn_into()?;
    let vert=shader(&gl,Gl::VERTEX_SHADER,"#version 300 es\nprecision highp float;out vec2 uv;void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));uv=p;gl_Position=vec4(p*2.0-1.0,0,1);}")?;
    let frag = shader(
        &gl,
        Gl::FRAGMENT_SHADER,
        r#"#version 300 es
 precision highp float;in vec2 uv;out vec4 color;uniform sampler2D field;uniform float lim;uniform float cursor;uniform float positive;uniform float logpower;
 const vec3 palette[16]=vec3[16](
vec3(0.050383,0.029803,0.527975),
vec3(0.200445,0.017902,0.593364),
vec3(0.312543,0.008239,0.635700),
vec3(0.417642,0.000564,0.658390),
vec3(0.517933,0.021563,0.654109),
vec3(0.610667,0.090204,0.619951),
vec3(0.692840,0.165141,0.564522),
vec3(0.764193,0.240396,0.502126),
vec3(0.826588,0.315714,0.441316),
vec3(0.881443,0.392529,0.383229),
vec3(0.928329,0.472975,0.326067),
vec3(0.965024,0.559118,0.268513),
vec3(0.988260,0.652325,0.211364),
vec3(0.994141,0.753137,0.161404),
vec3(0.977995,0.861432,0.142808),
vec3(0.940015,0.975158,0.131326));
 void main(){float raw=texture(field,uv).r;
 float z=raw/lim*8.0;float q=sign(z)*log(abs(z)+sqrt(z*z+1.0))/2.77647;
 float t=clamp(positive>0.5?q:0.5+0.5*q,0.0,1.0);
 if(logpower>0.5)t=clamp((log(max(raw,1e8))/log(10.0)-8.0)/5.0,0.0,1.0);
 float pos=t*15.0;int j=min(int(floor(pos)),14);vec3 c=mix(palette[j],palette[j+1],pos-float(j));
 if(isnan(raw)||(logpower>0.5&&raw<=0.0))c=vec3(.035,.070,.10);
 if(cursor>=0.0&&abs(uv.x-cursor)<.0014)c=mix(c,vec3(1.,1.,1.),.75);color=vec4(c,1.);}
 "#,
    )?;
    let program = gl
        .create_program()
        .ok_or_else(|| error("WebGL program allocation"))?;
    gl.attach_shader(&program, &vert);
    gl.attach_shader(&program, &frag);
    gl.link_program(&program);
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .as_bool()
        != Some(true)
    {
        return Err(error(gl.get_program_info_log(&program).unwrap_or_default()));
    }
    gl.use_program(Some(&program));
    let texture = gl
        .create_texture()
        .ok_or_else(|| error("WebGL texture allocation"))?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::NEAREST as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::NEAREST as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
    let a = js_sys::Float32Array::from(v);
    gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_array_buffer_view(
        Gl::TEXTURE_2D,
        0,
        Gl::R32F as i32,
        width as i32,
        height as i32,
        0,
        Gl::RED,
        Gl::FLOAT,
        Some(&a),
    )?;
    let limit = fixed_limit.unwrap_or_else(|| percentile(v, p));
    gl.uniform1f(gl.get_uniform_location(&program, "lim").as_ref(), limit);
    gl.uniform1f(
        gl.get_uniform_location(&program, "logpower").as_ref(),
        if id == "power" { 1. } else { 0. },
    );
    gl.uniform1f(
        gl.get_uniform_location(&program, "positive").as_ref(),
        if positive { 1. } else { 0. },
    );
    gl.uniform1f(
        gl.get_uniform_location(&program, "cursor").as_ref(),
        cursor.unwrap_or(-1.),
    );
    gl.viewport(0, 0, canvas.width() as i32, canvas.height() as i32);
    gl.draw_arrays(Gl::TRIANGLES, 0, 3);
    gl.delete_texture(Some(&texture));
    gl.delete_program(Some(&program));
    gl.delete_shader(Some(&vert));
    gl.delete_shader(Some(&frag));
    if gl.get_error() != Gl::NO_ERROR {
        return Err(error(format!("WebGL rendering error: {id}")));
    }
    if id == "power" {
        el("power-scale").set_inner_html("<span>10⁸</span><span class='colorbar' aria-label='Plasma color scale'></span><span>10¹³ m⁻³</span><span>log · fixed; dark = nonpositive</span>");
        return Ok(());
    }
    let low = if positive { 0. } else { -limit };
    let units = if id == "power" {
        "m⁻³"
    } else {
        "relative spectral units"
    };
    el(&format!("{id}-scale")).set_inner_html(&format!("<span>{low:.2e}</span><span class='colorbar' aria-label='Plasma color scale'></span><span>{limit:.2e} {units}</span><span>asinh · fixed during playback</span>"));
    Ok(())
}
fn overview(a: &App) -> Result<(), JsValue> {
    let n = a.m.frames.len();
    let frame = &a.m.frames[a.selected];
    let cursor = Some(
        (((frame.start_unix + frame.end_unix) / 2. - a.view_start) / (a.view_end - a.view_start))
            as f32,
    );
    let dt = a.m.frames[0].end_unix - a.m.frames[0].start_unix;
    let width = ((a.view_end - a.view_start) / dt).ceil().max(1.) as usize;
    let columns: Vec<Option<usize>> = (0..width)
        .map(|i| {
            let t = a.view_start + (a.view_end - a.view_start) * (i as f64 + 0.5) / width as f64;
            let j = a.m.frames.partition_point(|f| f.end_unix <= t);
            a.m.frames.get(j).filter(|f| f.start_unix <= t).map(|_| j)
        })
        .collect();
    let axes = doc().get_elements_by_class_name("time-axis");
    for i in 0..axes.length() {
        axes.item(i).unwrap().set_inner_html(&format!(
            "<span>{}</span><span>UTC · gaps left empty</span><span>{}</span>",
            utc(a.view_start),
            utc(a.view_end)
        ));
    }
    text(
        "plasma-window",
        &format!("{}–{} UTC", utc(a.view_start), utc(a.view_end)),
    );
    let time: HtmlInputElement = el("time").dyn_into()?;
    time.set_min(
        &a.m.frames
            .partition_point(|f| f.end_unix <= a.view_start)
            .min(n - 1)
            .to_string(),
    );
    time.set_max(
        &a.m.frames
            .partition_point(|f| f.start_unix < a.view_end)
            .saturating_sub(1)
            .to_string(),
    );
    let data = if a.raw_view { &a.raw } else { &a.broad };
    // Convert band/frequency/time to band/time/frequency before frequency merging.
    let mut all_history = Vec::new();
    for gate in 0..4 {
        let mut bands = vec![0.; 2 * n * 100];
        for band in 0..2 {
            for t in 0..n {
                for f in 0..100 {
                    bands[(band * n + t) * 100 + f] = data[((band * 4 + gate) * 100 + f) * n + t];
                }
            }
        }
        let (merged, bins, low) =
            merge_bands(&bands, n, 100, a.m.frames[0].centers_hz, 25000.).map_err(error)?;
        let mut history = vec![0.; bins * n];
        for t in 0..n {
            for f in 0..bins {
                history[f * n + t] = merged[t * bins + f];
            }
        }
        all_history.push((history, bins, low));
    }
    let combined: Vec<f32> = all_history
        .iter()
        .flat_map(|(h, _, _)| h.iter().copied())
        .collect();
    let shared_limit = percentile(&combined, a.percentile);
    let bounds = [[47., 221.], [128., 302.], [209., 383.], [290., 464.]];
    let elevation = a.m.frames[a.selected].elevation_deg;
    for (gate, (history, bins, low)) in all_history.iter().enumerate() {
        let first =
            (((a.broad_frequency[0] - low) / 25000.).ceil().max(0.) as usize).min(*bins - 1);
        let last = (((a.broad_frequency[1] - low) / 25000.)
            .floor()
            .max(first as f64) as usize)
            .min(*bins - 1);
        text(
            &format!("broad-high-{gate}"),
            &format!("{:.3}", (low + last as f64 * 25000.) / 1e6),
        );
        text(
            &format!("broad-low-{gate}"),
            &format!("{:.3}", (low + first as f64 * 25000.) / 1e6),
        );
        let visible: Vec<f32> = (first..=last)
            .flat_map(|f| {
                columns
                    .iter()
                    .map(move |t| t.map_or(f32::NAN, |t| history[f * n + t]))
            })
            .collect();
        draw(
            &format!("broad{gate}"),
            &visible,
            width,
            last - first + 1,
            a.percentile,
            cursor,
            (Some(shared_limit), a.raw_view),
        )?;
        text(
            &format!("gate-altitude{gate}"),
            &format!(
                "{:.0}–{:.0} km altitude · broad gate {}",
                altitude(bounds[gate][0], elevation),
                altitude(bounds[gate][1], elevation),
                gate + 1
            ),
        );
    }
    let elevation = a.m.frames[0].elevation_deg;
    let shift = 0.;
    let minimum = a.profile_altitude[0];
    let indices: Vec<usize> =
        a.m.ion_range_km
            .iter()
            .enumerate()
            .filter_map(|(i, &r)| {
                let h = altitude(r - shift, elevation);
                if (minimum..=a.profile_altitude[1]).contains(&h) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
    let mut power = vec![];
    for &r in &indices {
        power.extend(
            columns
                .iter()
                .map(|t| t.map_or(f32::NAN, |t| a.power[r * n + t])),
        );
    }
    draw(
        "power",
        &power,
        width,
        indices.len(),
        a.percentile,
        cursor,
        (Some(1e13), true),
    )?;
    let low = altitude(a.m.ion_range_km[indices[0]] - shift, elevation);
    let high = altitude(
        a.m.ion_range_km[*indices.last().unwrap()] - shift,
        elevation,
    );
    text("power-axis-label", "Response-centroid altitude / km");
    text("power-axis-caption", "10 µs lag raw Ne / m⁻³ versus UTC");
    let r0 = a.m.ion_range_km[0];
    let half = 2.1724186 / 2.;
    text(
        "power-support",
        &format!(
            "First recorded gate response: {:.1}–{:.1} km altitude.",
            altitude(r0 - half, elevation),
            altitude(r0 + half, elevation)
        ),
    );
    let mut ticks = String::new();
    for h in (0..=1000).step_by(10) {
        let h = h as f64;
        if h < low || h > high {
            continue;
        }
        let top = 100. * (high - h) / (high - low);
        if (h as usize).is_multiple_of(50) {
            ticks += &format!("<span class='major' style='top:{top:.3}%'>{h:.0}</span>");
        } else {
            ticks += &format!("<span class='minor' style='top:{top:.3}%'></span>");
        }
    }
    el("power-ticks").set_inner_html(&ticks);
    profile(a, &indices);
    Ok(())
}
fn profile(a: &App, indices: &[usize]) {
    let n = a.m.frames.len();
    let f = &a.m.frames[a.selected];
    let [bottom, top] = a.profile_altitude;
    let [xmin, xmax] = a.profile_power;
    let x = |v: f64| {
        52. + 280. * (v.max(xmin).min(xmax).log10() - xmin.log10()) / (xmax / xmin).log10()
    };
    let y = |h: f64| 310. - 280. * (h - bottom) / (top - bottom);
    let mut svg=String::from("<svg preserveAspectRatio='none' viewBox='0 0 360 360' role='img' aria-label='Signed 10 microsecond lag raw electron density versus response-centroid altitude, logarithmic scale 10^8 to 10^13 per cubic metre, with one sigma uncertainty'><g font-family='monospace' font-size='10' fill='#8b9aa8'>");
    let tick = (((top - bottom) / 7. / 5.).ceil() * 5.).max(1.) as i32;
    for h in
        ((bottom / tick as f64).ceil() as i32 * tick..=top.floor() as i32).step_by(tick as usize)
    {
        let yy = y(h as f64);
        if true {
            svg += &format!(
                "<path d='M52 {yy} H332' stroke='#293743'/><text x='12' y='{}'>{h}</text>",
                yy + 4.
            );
        } else {
            svg += &format!("<path d='M46 {yy} H52' stroke='#72878e'/>");
        }
    }
    for exp in xmin.log10().ceil() as i32..=xmax.log10().floor() as i32 {
        let xx = x(10f64.powi(exp));
        svg += &format!("<path d='M{xx} 30 V310' stroke='#293743'/>");
        if let Ok(label) = math_power(&exp.to_string(), xx, 332., "middle") {
            svg += &label;
        }
    }
    let mut path = String::new();
    let mut connected = false;
    let mut nonpositive = 0;
    for &i in indices {
        let h = altitude(a.m.ion_range_km[i], f.elevation_deg);
        if h < bottom || h > top {
            continue;
        }
        let v = a.power[i * n + a.selected] as f64;
        let se = a.power_sigma[i * n + a.selected] as f64;
        let yy = y(altitude(a.m.ion_range_km[i], f.elevation_deg));
        let lo = v - se;
        let hi = v + se;
        if hi > xmin && lo < xmax {
            svg+=&format!("<path d='M{} {yy:.2} H{}' stroke='#80dfce' stroke-opacity='.35'><title>Raw Ne {v:.3e} ± {se:.3e} m⁻³ (1σ)</title></path>",x(lo),x(hi));
        }
        if v <= 0. {
            nonpositive += 1;
        }
        if !(xmin..=xmax).contains(&v) {
            connected = false;
            let xx = if v < xmin { 52. } else { 332. };
            svg+=&format!("<path d='M{xx} {yy:.2} l4 -2 v4 z' fill='#e8ac69'><title>Outside logarithmic display: {v:.3e} ± {se:.3e} m⁻³</title></path>");
        } else {
            let op = if connected { "L" } else { "M" };
            path += &format!("{op}{:.2} {yy:.2} ", x(v));
            connected = true;
            svg += &format!("<circle cx='{}' cy='{yy:.2}' r='1.3' fill='#80dfce'><title>Raw Ne {v:.3e} ± {se:.3e} m⁻³</title></circle>", x(v));
        }
    }
    svg+=&format!("<path d='{path}' stroke='#80dfce' fill='none' stroke-width='1.4'/><text x='190' y='352' text-anchor='middle'>Raw Ne / m⁻³ · log scale</text></g></svg>");
    el("profile").set_inner_html(&svg);
    if let Some(c) = &a.calibration {
        if let Ok(c) = c.try_borrow() {
            calibration::markers(a, &c);
        }
    }
    text("power-uncertainty",&format!("Horizontal bars: approximate ±1σ from 14 short integrations plus calibration. Clutter-induced covariance and systematic errors are not included. {nonpositive} nonpositive estimates in the displayed altitude interval; edge markers show values outside the log axis. Signed estimates remain in HDF5."));
}
fn decoded(a: &mut App) -> Result<(), JsValue> {
    if a.decoded_selected != Some(a.selected) {
        return Ok(());
    }
    let bins = guisdap_core::NFFT;
    let mut data = a.decoded.clone();
    if a.magnitude {
        for x in &mut data {
            *x = x.abs();
        }
    }
    if a.smoothing > 1 {
        let source = data.clone();
        for (dest, row) in data.chunks_mut(bins).zip(source.chunks(bins)) {
            for (i, v) in dest.iter_mut().enumerate() {
                let left = i.saturating_sub(a.smoothing / 2);
                let right = (i + (a.smoothing - 1) / 2 + 1).min(bins);
                *v = row[left..right].iter().sum::<f32>() / (right - left) as f32;
            }
        }
    }
    if a.magnitude {
        // Detection contrast, as in ipypl: remove frequency-dependent noise amplitude.
        // This is explicitly not a calibrated spectrum or a power estimate.
        for b in 0..2 {
            for f in 0..bins {
                let mut column: Vec<f32> = (0..90).map(|r| data[(b * 90 + r) * bins + f]).collect();
                column.sort_by(f32::total_cmp);
                let median = ((column[44] + column[45]) * 0.5).max(1e-20);
                for r in 0..90 {
                    data[(b * 90 + r) * bins + f] /= median;
                }
            }
        }
    }
    let frame = &a.m.frames[a.selected];
    let (merged, width, low) =
        merge_bands(&data, 90, bins, frame.centers_hz, 2.5e6 / bins as f64).map_err(error)?;
    let limit = *a.limit.get_or_insert_with(|| {
        if a.magnitude {
            6.
        } else {
            percentile(&merged, a.percentile)
        }
    });
    draw(
        "decoded",
        &merged,
        width,
        90,
        a.percentile,
        None,
        (Some(limit), a.magnitude),
    )?;
    text(
        "decoded-range",
        &format!("{} — {} UTC", utc(frame.start_unix), utc(frame.end_unix)),
    );
    text("freq-min", &format!("{:.3}", low / 1e6));
    text(
        "freq-max",
        &format!(
            "{:.3}",
            (low + (width - 1) as f64 * 2.5e6 / bins as f64) / 1e6
        ),
    );
    for (class, i) in [("alt-min", 0), ("alt-max", 89)] {
        let collection = doc().get_elements_by_class_name(class);
        for k in 0..collection.length() {
            collection.item(k).unwrap().set_text_content(Some(&format!(
                "{:.0}",
                altitude(a.m.decoded_range_km[i], frame.elevation_deg)
            )));
        }
    }
    Ok(())
}
async fn select(state: State, index: usize) -> Result<(), JsValue> {
    let (url, request, cached) = {
        let mut a = state.borrow_mut();
        a.request += 1;
        a.busy = true;
        text("loading", "Loading next integration…");
        (
            format!("{}frames/{}/native-long.f32", a.root, a.m.frames[index].id),
            a.request,
            a.cache
                .iter()
                .find(|(i, _)| *i == index)
                .map(|(_, v)| v.clone()),
        )
    };
    let result = match cached {
        Some(v) => Ok(v),
        None => floats(&url).await,
    };
    let mut a = state.borrow_mut();
    if a.request != request {
        return Ok(());
    }
    a.busy = false;
    text("loading", "");
    let data = match result {
        Ok(d) => d,
        Err(e) => {
            return Err(e);
        }
    };
    if data.len() != 2 * 800 * 90 * 2 {
        return Err(error("invalid native lag dimensions"));
    }
    a.decoded = native_spectra(&data, a.weighted).map_err(error)?;
    if !a.cache.iter().any(|(i, _)| *i == index) {
        a.cache.push_back((index, data.clone()));
    }
    while a.cache.len() > 6 {
        a.cache.pop_front();
    }
    a.native = data;
    a.selected = index;
    a.decoded_selected = Some(index);
    let f = &a.m.frames[index];
    text(
        "selected",
        &format!("{} — {} UTC", utc(f.start_unix), utc(f.end_unix)),
    );
    text(
        "frameinfo",
        &format!(
            "Integration {} / {} · {} / {:.0} native records · {:.2} MW",
            index + 1,
            a.m.frames.len(),
            f.source_count.max(1),
            ((f.end_unix - f.start_unix) / 5.).max(1.),
            f.transmitter_w / 1e6
        ),
    );
    text(
        "pointing",
        &format!("{:.1}° / {:.1}°", f.azimuth_deg, f.elevation_deg),
    );
    text(
        "profile-time",
        &format!("{} — {} UTC", utc(f.start_unix), utc(f.end_unix)),
    );
    el("time")
        .dyn_into::<HtmlInputElement>()?
        .set_value(&index.to_string());
    // Commit all panels together after fetch and FFT; never display mixed timestamps.
    overview(&a)?;
    decoded(&mut a)?;
    Ok(())
}
fn on(
    id: &str,
    event: &str,
    callback: impl FnMut(web_sys::Event) + 'static,
) -> Result<(), JsValue> {
    let c = Closure::<dyn FnMut(web_sys::Event)>::new(callback);
    el(id).add_event_listener_with_callback(event, c.as_ref().unchecked_ref())?;
    c.forget();
    Ok(())
}
#[wasm_bindgen]
pub async fn start() -> Result<(), JsValue> {
    let cfg: Config = json("config.json").await?;
    let query = web_sys::window().unwrap().location().search()?;
    let param = |key: &str| {
        query.trim_start_matches('?').split('&').find_map(|part| {
            part.strip_prefix(&format!("{key}="))
                .and_then(|v| v.parse::<f64>().ok())
        })
    };
    let chosen = param("resolution").unwrap_or(0.) as usize;
    if let Some(selector) = doc().get_element_by_id("plasma-resolution") {
        selector.set_inner_html(
            &cfg.datasets
                .iter()
                .enumerate()
                .map(|(i, d)| format!("<option value='{i}'>{}</option>", d.label))
                .collect::<String>(),
        );
        selector
            .clone()
            .dyn_into::<HtmlSelectElement>()?
            .set_value(&chosen.to_string());
    }
    let data_root = cfg
        .datasets
        .get(chosen)
        .map_or(cfg.data_root.as_str(), |d| d.data_root.as_str())
        .to_string();
    let root = if data_root.ends_with('/') {
        data_root
    } else {
        data_root + "/"
    };
    let m: Manifest = json(&(root.clone() + "index.json")).await?;
    if m.schema != "beata22-quicklook-v5"
        || m.frames.is_empty()
        || m.ion_range_km.len() != 431
        || m.decoded_range_km.len() != 90
    {
        return Err(error("unsupported or empty dataset"));
    }
    let broad = floats(&(root.clone() + &m.overview_root + "broad-excess.f32")).await?;
    let raw = floats(&(root.clone() + &m.overview_root + "broad-raw.f32")).await?;
    let power = floats(&(root.clone() + &m.overview_root + "ion-short-raw-ne.f32")).await?;
    let power_sigma =
        floats(&(root.clone() + &m.overview_root + "ion-short-raw-ne-sigma.f32")).await?;
    if let Some(button) = doc().get_element_by_id("refresh-data") {
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let _ = web_sys::window().unwrap().location().reload();
        });
        button.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    let n = m.frames.len();
    if broad.len() != 800 * n
        || raw.len() != 800 * n
        || power.len() != 431 * n
        || power_sigma.len() != 431 * n
    {
        return Err(error("invalid overview dimensions"));
    }
    let first = &m.frames[0];
    let last = m.frames.last().unwrap();
    text(
        "window",
        &format!("{} – {}", utc(first.start_unix), utc(last.end_unix)),
    );
    let dt = first.end_unix - first.start_unix;
    text("cadence", &format!("{dt:.1} s · {n} records"));
    let gaps = m
        .frames
        .windows(2)
        .filter(|w| (w[1].start_unix - w[0].end_unix).abs() > 0.1)
        .count();
    text(
        "status",
        &format!("{n} observed integrations · {gaps} time gaps"),
    );
    let axes = doc().get_elements_by_class_name("time-axis");
    for k in 0..axes.length() {
        axes.item(k).unwrap().set_inner_html(&format!(
            "<span>{}</span><span>UTC · one column per integration</span><span>{}</span>",
            utc(first.start_unix),
            utc(last.end_unix)
        ));
    }
    el("time")
        .dyn_into::<HtmlInputElement>()?
        .set_max(&(n - 1).to_string());
    let target = param("time").unwrap_or(1667408700.1);
    let initial = m
        .frames
        .partition_point(|f| f.end_unix <= target)
        .min(n - 1);
    let view_start = param("from")
        .unwrap_or(first.start_unix)
        .clamp(first.start_unix, last.end_unix - dt);
    let view_end = param("to")
        .unwrap_or(last.end_unix)
        .clamp(view_start + dt, last.end_unix);
    let state = Rc::new(RefCell::new(App {
        calibration: None,
        profile_altitude: [40., 400.],
        profile_power: [1e8, 1e13],
        broad_frequency: [-6.15e6, -1.275e6],
        root,
        m,
        broad,
        raw,
        power,
        power_sigma,
        decoded: vec![],
        selected: initial,
        view_start,
        view_end,
        decoded_selected: None,
        raw_view: false,
        percentile: 0.995,
        busy: false,
        request: 0,
        smoothing: 4,
        magnitude: true,
        weighted: true,
        limit: None,
        native: vec![],
        cache: std::collections::VecDeque::new(),
    }));
    if doc().get_element_by_id("plasma-resolution").is_some() {
        let s = state.clone();
        on("plasma-resolution", "change", move |_| {
            let a = s.borrow();
            let resolution = el("plasma-resolution")
                .dyn_into::<HtmlSelectElement>()
                .unwrap()
                .value();
            let _ = web_sys::window().unwrap().location().set_search(&format!(
                "?resolution={resolution}&from={}&to={}&time={}",
                a.view_start,
                a.view_end,
                a.m.frames[a.selected].start_unix + 0.1
            ));
        })?;
    }
    for (id, factor) in [
        ("plasma-zoom-in", 0.5),
        ("plasma-zoom-out", 2.),
        ("plasma-view-all", 0.),
    ] {
        let s = state.clone();
        on(id, "click", move |_| {
            let mut a = s.borrow_mut();
            let first = a.m.frames[0].start_unix;
            let last = a.m.frames.last().unwrap().end_unix;
            let dt = a.m.frames[0].end_unix - first;
            let center = (a.m.frames[a.selected].start_unix + a.m.frames[a.selected].end_unix) / 2.;
            let width = if factor == 0. {
                last - first
            } else {
                ((a.view_end - a.view_start) * factor).clamp(dt, last - first)
            };
            a.view_start = (center - width / 2.).clamp(first, last - width);
            a.view_end = a.view_start + width;
            if let Err(e) = overview(&a) {
                report(e);
            }
        })?;
    }
    let s = state.clone();
    on("time", "input", move |_| {
        let s = s.clone();
        let i = el("time")
            .dyn_into::<HtmlInputElement>()
            .unwrap()
            .value()
            .parse::<usize>()
            .unwrap_or(0)
            .min(s.borrow().m.frames.len() - 1);
        spawn_local(async move {
            if let Err(e) = select(s, i).await {
                report(e);
            }
        });
    })?;
    for id in ["baseline", "stretch", "mode", "smooth", "weight"] {
        let s = state.clone();
        on(id, "change", move |_| {
            let mut a = s.borrow_mut();
            a.raw_view = el("baseline")
                .dyn_into::<HtmlSelectElement>()
                .unwrap()
                .value()
                == "raw";
            a.percentile = match el("stretch")
                .dyn_into::<HtmlSelectElement>()
                .unwrap()
                .value()
                .as_str()
            {
                "99" => 0.99,
                "999" => 0.999,
                "100" => 1.,
                _ => 0.995,
            };
            a.magnitude =
                el("mode").dyn_into::<HtmlSelectElement>().unwrap().value() == "magnitude";
            a.smoothing = el("smooth")
                .dyn_into::<HtmlSelectElement>()
                .unwrap()
                .value()
                .parse()
                .unwrap_or(4);
            a.weighted = el("weight")
                .dyn_into::<HtmlSelectElement>()
                .unwrap()
                .value()
                == "sqrt";
            if matches!(id, "mode" | "smooth" | "weight" | "stretch") {
                a.limit = None;
            }
            if id == "weight" && !a.native.is_empty() {
                match native_spectra(&a.native, a.weighted) {
                    Ok(d) => a.decoded = d,
                    Err(e) => {
                        report(error(e));
                        return;
                    }
                }
            }
            if let Err(e) = overview(&a).and_then(|_| decoded(&mut a)) {
                report(e);
            }
        })?;
    }
    select(state.clone(), initial).await?;
    calibration::start(state.clone())?;
    Ok(())
}

fn math_power(exponent: &str, x: f64, y: f64, anchor: &str) -> Result<String, JsValue> {
    let mj = js_sys::Reflect::get(&web_sys::window().unwrap(), &"MathJax".into())?;
    let convert: js_sys::Function = js_sys::Reflect::get(&mj, &"tex2svg".into())?.dyn_into()?;
    let node: web_sys::Element = convert
        .call1(
            &mj,
            &format!(
                "10^{{{}}}",
                exponent.strip_suffix(".00").unwrap_or(exponent)
            )
            .into(),
        )?
        .dyn_into()?;
    let svg = node
        .query_selector("svg")?
        .ok_or_else(|| error("MathJax SVG missing"))?;
    let width = if exponent.ends_with(".00") || !exponent.contains('.') {
        30.
    } else {
        42.
    };
    let left = x - if anchor == "middle" {
        width / 2.
    } else if anchor == "end" {
        width
    } else {
        0.
    };
    svg.set_attribute("x", &left.to_string())?;
    svg.set_attribute("y", &(y - 15.).to_string())?;
    svg.set_attribute("width", &width.to_string())?;
    svg.set_attribute("height", "18")?;
    svg.set_attribute("style", "color:#b6c8d8;overflow:visible")?;
    Ok(svg.outer_html())
}
