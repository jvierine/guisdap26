use super::*;

pub(super) fn spectrum(v: &Viewer) -> String {
    let g = &selected(v).gates[v.gate];
    let mut s = svg(620., 330.);
    let frequency: Vec<_> = (0..=800).map(|i| -40000. + i as f64 * 100.).collect();
    let lags: Vec<_> = g.lag_us.iter().map(|t| t * 1e-6).collect();
    // BEATA's exported lag grid is 10 us. Reject incompatible products rather
    // than silently deriving a different normalization from a sparse subset.
    let estimate = guisdap_ion::dft::AcfDft::new(&lags, &frequency, 10e-6).and_then(|op| {
        let values = op.transform(&g.observed)?;
        let errors = op.diagonal_standard_error(&g.sigma)?;
        Ok((op, values, errors))
    });
    let Ok((op, values, errors)) = estimate else {
        label(
            &mut s,
            50.,
            80.,
            "Measured ACF spectrum unavailable",
            "start",
        );
        s.push_str("</svg>");
        return s;
    };
    let fitted = g
        .result
        .as_ref()
        .and_then(|f| op.transform(&f.predicted).ok());
    let low = values
        .iter()
        .zip(&errors)
        .map(|(a, b)| a - b)
        .chain(fitted.iter().flatten().copied())
        .fold(0., f64::min);
    let high = values
        .iter()
        .zip(&errors)
        .map(|(a, b)| a + b)
        .chain(fitted.iter().flatten().copied())
        .fold(1., f64::max);
    let pad = (high - low) * 0.06;
    let scale = 10f64.powf(high.abs().max(low.abs()).max(1.).log10().floor());
    let x = |f: f64| 60. + 510. * (f + 40000.) / 80000.;
    let y = |a: f64| 260. - 210. * (a - low + pad) / (high - low + 2. * pad);
    for tick in [-40, -20, 0, 20, 40] {
        let xx = x(tick as f64 * 1000.);
        line(&mut s, xx, 50., xx, 260., "#293d4c");
        label(&mut s, xx, 280., &tick.to_string(), "middle");
    }
    for i in 0..=4 {
        let a = low + (high - low) * i as f64 / 4.;
        line(&mut s, 60., y(a), 570., y(a), "#293d4c");
        label(&mut s, 55., y(a) + 4., &format!("{:.2}", a / scale), "end");
    }
    line(&mut s, 60., y(0.), 570., y(0.), "#526170");
    label(
        &mut s,
        60.,
        18.,
        &format!("ACF DFT / (m⁻³ Hz⁻¹) · ×{scale:.0e}"),
        "start",
    );
    label(
        &mut s,
        60.,
        36.,
        &format!(
            "{} measured lags · {} missing · {}",
            op.unique_lags,
            op.missing_lags,
            if op.has_zero_lag {
                "zero lag measured"
            } else {
                "baseline undetermined"
            }
        ),
        "start",
    );
    label(&mut s, 315., 305., "Frequency offset / kHz", "middle");
    let mut band = String::new();
    for i in 0..values.len() {
        band.push_str(&format!(
            "{}{},{} ",
            if i == 0 { "M" } else { "L" },
            x(frequency[i]),
            y(values[i] + errors[i])
        ));
    }
    for i in (0..values.len()).rev() {
        band.push_str(&format!(
            "L{},{} ",
            x(frequency[i]),
            y(values[i] - errors[i])
        ));
    }
    band.push('Z');
    s.push_str(&format!(
        "<path d='{band}' fill='#96a9bb' fill-opacity='0.30'/>"
    ));
    for (data, color) in [(Some(&values), "#b8c8d8"), (fitted.as_ref(), "#efbc50")] {
        if let Some(data) = data {
            let path = data
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    format!(
                        "{}{},{} ",
                        if i == 0 { "M" } else { "L" },
                        x(frequency[i]),
                        y(*a)
                    )
                })
                .collect::<String>();
            s.push_str(&format!(
                "<path d='{path}' fill='none' stroke='{color}' stroke-width='2'/>"
            ));
        }
    }
    s.push_str("</svg>");
    s
}
