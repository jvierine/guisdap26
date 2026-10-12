//! Per-colorbar controls; display limits stay in the viewer, never in data products.
use super::*;
const NAMES: [&str; 5] = [
    "Electron density / m⁻³",
    "Ion temperature / K",
    "Electron temperature / K",
    "Velocity / m s⁻¹",
    "Raw Ne / m⁻³",
];
fn active() -> Option<usize> {
    el("fit-colors")
        .get_attribute("data-param")
        .and_then(|s| s.parse().ok())
        .filter(|p| *p < 5)
}
fn logarithmic(param: usize) -> bool {
    param == 0 || param == 4
}
fn coordinate(param: usize, value: f64) -> f64 {
    if logarithmic(param) {
        value.log10()
    } else {
        value
    }
}
fn physical(param: usize, value: f64) -> f64 {
    if logarithmic(param) {
        10_f64.powf(value)
    } else {
        value
    }
}
pub(super) fn sync(v: &Viewer) {
    let Some(param) = active() else { return };
    for edge in 0..2 {
        let value = v.scales[param][edge];
        let input = el(&format!("fit-color-{edge}"))
            .dyn_into::<HtmlInputElement>()
            .unwrap();
        input.set_value(&coordinate(param, value).to_string());
        text(
            &format!("fit-color-value-{edge}"),
            &if logarithmic(param) {
                format!("{value:.3e} m⁻³")
            } else if param == 3 {
                format!("{value:.0} m/s")
            } else {
                format!("{value:.0} K")
            },
        );
        el(&format!("fit-scale-{param}-{edge}"))
            .dyn_into::<HtmlInputElement>()
            .unwrap()
            .set_value(&value.to_string());
    }
}
fn close() {
    if let Some(param) = active() {
        el(&format!("fit-colorbar-{param}"))
            .set_attribute("aria-expanded", "false")
            .unwrap();
    }
    el("fit-colors").set_attribute("hidden", "").unwrap();
    el("fit-colors").remove_attribute("data-param").unwrap();
}
pub(super) fn install(state: Rc<RefCell<Viewer>>) -> Result<(), JsValue> {
    for (param, &name) in NAMES.iter().enumerate() {
        let state = state.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            close();
            let v = state.borrow();
            let panel = el("fit-colors");
            panel
                .set_attribute("data-param", &param.to_string())
                .unwrap();
            panel.remove_attribute("hidden").unwrap();
            el(&format!("fit-colorbar-{param}"))
                .set_attribute("aria-expanded", "true")
                .unwrap();
            text("fit-colors-title", name);
            text(
                "fit-colors-mode",
                if logarithmic(param) {
                    "Logarithmic density scale"
                } else if param == 3 {
                    "Seismic scale; white stays at zero"
                } else {
                    "Linear temperature scale"
                },
            );
            for edge in 0..2 {
                let input = el(&format!("fit-color-{edge}"))
                    .dyn_into::<HtmlInputElement>()
                    .unwrap();
                let current = v.scales[param];
                let (lo, hi, step) = if logarithmic(param) {
                    (
                        7_f64.min(current[0].log10()),
                        14_f64.max(current[1].log10()),
                        0.01,
                    )
                } else if param == 3 && edge == 0 {
                    ((-5000_f64).min(current[0]), -1., 1.)
                } else if param == 3 {
                    (1., 5000_f64.max(current[1]), 1.)
                } else {
                    (0_f64.min(current[0]), 20000_f64.max(current[1]), 10.)
                };
                input.set_min(&lo.to_string());
                input.set_max(&hi.to_string());
                input.set_step(&step.to_string());
            }
            sync(&v);
            let r = el(&format!("fit-colorbar-{param}")).get_bounding_client_rect();
            let window = web_sys::window().unwrap();
            let width = window
                .inner_width()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(1024.);
            let height = window
                .inner_height()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(768.);
            let pr = panel.get_bounding_client_rect();
            let x = (r.left() - pr.width() - 12.).clamp(8., (width - pr.width() - 8.).max(8.));
            let y = r.top().clamp(8., (height - pr.height() - 8.).max(8.));
            panel
                .set_attribute("style", &format!("left:{x}px;top:{y}px"))
                .unwrap();
        });
        el(&format!("fit-colorbar-{param}"))
            .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    for edge in 0..2 {
        let state = state.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let Some(param) = active() else { return };
            let input = el(&format!("fit-color-{edge}"))
                .dyn_into::<HtmlInputElement>()
                .unwrap();
            let Ok(mut coordinate_value) = input.value().parse::<f64>() else {
                return;
            };
            let mut v = state.borrow_mut();
            let step = input.step().parse::<f64>().unwrap_or(1.);
            let other = coordinate(param, v.scales[param][1 - edge]);
            coordinate_value = if edge == 0 {
                coordinate_value.min(other - step)
            } else {
                coordinate_value.max(other + step)
            };
            v.scales[param][edge] = physical(param, coordinate_value);
            v.map_key.set(None);
            sync(&v);
            if let Err(e) = time_altitude(&v, param) {
                web_sys::console::error_1(&e);
            }
        });
        el(&format!("fit-color-{edge}"))
            .add_event_listener_with_callback("input", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        close();
    });
    el("fit-colors-close")
        .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
    cb.forget();
    let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        if e.key() == "Escape" {
            close();
        }
    });
    el("fit-colors").add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())?;
    cb.forget();
    Ok(())
}
