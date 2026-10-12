//! Instrument diagnostics retain native time coverage and share the fit time window.
use super::*;
#[derive(Deserialize)]
pub(super) struct Data {
    records: Vec<Record>,
}
#[derive(Deserialize)]
struct Record {
    start_unix: f64,
    end_unix: f64,
    system_temperature_k: Option<f64>,
    transmitter_w: f64,
    azimuth_deg: f64,
    elevation_deg: f64,
}
impl Record {
    fn value(&self, column: usize) -> Option<f64> {
        match column {
            0 => self.system_temperature_k,
            1 => Some(self.transmitter_w / 1e6),
            2 => Some(self.azimuth_deg),
            _ => Some(self.elevation_deg),
        }
        .filter(|v| v.is_finite())
    }
}
pub(super) fn render(v: &Viewer) {
    let Some(data) = &v.instruments else {
        text(
            "fit-instrument-summary",
            "Instrument diagnostics unavailable.",
        );
        return;
    };
    let selected = &v.data.profiles[v.time];
    let mut summary = Vec::new();
    for (column, title) in [
        "System noise temperature / K",
        "Transmit power / MW",
        "Azimuth / degrees",
        "Elevation / degrees",
    ]
    .iter()
    .enumerate()
    {
        let values: Vec<_> = data
            .records
            .iter()
            .filter(|r| r.end_unix > v.window_start && r.start_unix < v.window_end)
            .filter_map(|r| r.value(column).filter(|a| column != 0 || *a > 0.))
            .collect();
        let mut s = svg(1100., 150.);
        label(
            &mut s,
            60.,
            18.,
            &format!("{title}{}", if column == 0 { " · logarithmic" } else { "" }),
            "start",
        );
        let transform = |a: f64| if column == 0 { a.log10() } else { a };
        if values.is_empty() {
            label(&mut s, 60., 60., "No valid measurements", "start");
        } else {
            let lo = values
                .iter()
                .copied()
                .map(transform)
                .fold(f64::INFINITY, f64::min);
            let hi = values
                .iter()
                .copied()
                .map(transform)
                .fold(f64::NEG_INFINITY, f64::max);
            let pad = ((hi - lo) * 0.1).max(hi.abs() * 0.002).max(0.01);
            let (lo, hi) = (lo - pad, hi + pad);
            let x = |t: f64| 60. + 890. * (t - v.window_start) / (v.window_end - v.window_start);
            let y = |a: f64| 110. - 75. * (a - lo) / (hi - lo);
            for i in 0..=2 {
                let value = lo + (hi - lo) * i as f64 / 2.;
                line(&mut s, 60., y(value), 950., y(value), "#293d4c");
                label(
                    &mut s,
                    55.,
                    y(value) + 4.,
                    &format!(
                        "{:.2}",
                        if column == 0 {
                            10_f64.powf(value)
                        } else {
                            value
                        }
                    ),
                    "end",
                );
            }
            for i in 0..=4 {
                let t = v.window_start + (v.window_end - v.window_start) * i as f64 / 4.;
                label(&mut s, x(t), 135., &clock(t), "middle");
            }
            for r in &data.records {
                if r.end_unix <= v.window_start || r.start_unix >= v.window_end {
                    continue;
                }
                if let Some(value) = r.value(column).filter(|a| column != 0 || *a > 0.) {
                    let value = transform(value);
                    s.push_str(&format!(
                        "<circle cx='{}' cy='{}' r='1' fill='#efbc50'/>",
                        x(((r.start_unix + r.end_unix) / 2.).clamp(v.window_start, v.window_end)),
                        y(value)
                    ));
                    line(
                        &mut s,
                        x(r.start_unix.max(v.window_start)),
                        y(value),
                        x(r.end_unix.min(v.window_end)),
                        y(value),
                        "#efbc50",
                    );
                }
            }
            let cursor = (selected.start_unix + selected.end_unix) / 2.;
            if cursor >= v.window_start && cursor <= v.window_end {
                line(&mut s, x(cursor), 30., x(cursor), 110., "#a9fff3");
            }
        }
        s.push_str("</svg>");
        el(&format!("fit-instrument-{column}")).set_inner_html(&s);
        let mut sum = 0.;
        let mut duration = 0.;
        for r in &data.records {
            let dt =
                (r.end_unix.min(selected.end_unix) - r.start_unix.max(selected.start_unix)).max(0.);
            if let Some(value) = r.value(column) {
                sum += dt * value;
                duration += dt;
            }
        }
        summary.push(if duration > 0. {
            format!(
                "{title}: {:.2} ({} s covered)",
                sum / duration,
                duration.round()
            )
        } else {
            format!("{title}: unavailable")
        });
    }
    text(
        "fit-instrument-summary",
        &format!("Selected interval · {}", summary.join(" · ")),
    );
}
