//! Calendar selection and queue controls, compiled to WebAssembly.
use super::{doc, el, error, json, text};
use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{HtmlInputElement, HtmlSelectElement};
#[derive(Clone, Deserialize)]
struct Product {
    label: String,
    link: String,
    complete: bool,
}
#[derive(Clone, Deserialize)]
struct Entry {
    #[serde(default)]
    calibration: Option<Calibration>,
    id: String,
    date: String,
    experiment: String,
    radar: String,
    sources: Vec<String>,
    hours: Vec<String>,
    products: Vec<Product>,
    supported: bool,
}
#[derive(Clone, Deserialize)]
struct Calibration {
    revision: u64,
    magic_constant: f64,
    included: usize,
    total: usize,
}
#[derive(Default, Deserialize)]
struct Inventory {
    updated: String,
    scanning: bool,
    directories: usize,
    errors: usize,
    skipped_at_depth: usize,
    sources: Vec<String>,
    experiments: Vec<Entry>,
}
#[derive(Deserialize)]
struct Job {
    #[serde(default)]
    calibration: Option<Calibration>,
    id: String,
    date: String,
    experiment: String,
    state: String,
    stage: String,
    completed: usize,
    total: usize,
    cores: usize,
    message: String,
}
struct View {
    inventory: Inventory,
    month: String,
    date: String,
    filter: String,
    selected: Option<String>,
    tab: String,
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn input(id: &str) -> HtmlInputElement {
    el(id).dyn_into().unwrap()
}
fn matches(v: &View, e: &Entry) -> bool {
    format!("{} {}", e.experiment, e.radar)
        .to_lowercase()
        .contains(&v.filter.to_lowercase())
}
fn render(v: &View) {
    text("catalog-status",&format!("{} experiment-days · {} directories checked · {} · {} read errors · {} depth-limited directories. {}",v.inventory.experiments.len(),v.inventory.directories,if v.inventory.scanning{"Scanning archive"}else{"Scan complete"},v.inventory.errors,v.inventory.skipped_at_depth,v.inventory.updated));
    text("catalog-sources",&format!("Sources: {}. UTC dates from archive directory names; recordings are validated during processing.",v.inventory.sources.join(", ")));
    let Ok(first) = NaiveDate::parse_from_str(&format!("{}-01", v.month), "%Y-%m-%d") else {
        return;
    };
    let start = first - Duration::days(first.weekday().num_days_from_monday() as i64);
    let mut html = String::new();
    for i in 0..42 {
        let day = start + Duration::days(i);
        let date = day.to_string();
        let entries: Vec<_> = v
            .inventory
            .experiments
            .iter()
            .filter(|e| e.date == date && matches(v, e))
            .collect();
        let processed = entries.iter().filter(|e| !e.products.is_empty()).count();
        html.push_str(&format!("<button class='day {} {}' data-date='{date}' aria-label='{date}: {} experiments, {processed} with products' aria-pressed='{}'><b>{}</b><span>{}</span><small>{}</small></button>",if day.month()==first.month(){""}else{"outside"},if date==v.date{"selected"}else{""},entries.len(),date==v.date,day.day(),if entries.is_empty(){String::new()}else{format!("{} experiments",entries.len())},if processed>0{format!("{processed} processed")}else{String::new()}));
    }
    el("calendar").set_inner_html(&html);
    text("selected-day", &v.date);
    let entries: Vec<_> = v
        .inventory
        .experiments
        .iter()
        .filter(|e| e.date == v.date && matches(v, e))
        .collect();
    let html = if entries.is_empty() {
        "<p>No experiments found for this date and filter. Inventory may still be scanning.</p>"
            .into()
    } else {
        entries.iter().map(|e|format!("<button class='experiment {}' data-experiment='{}'><strong>{}</strong><span>{} · {} hour directories · {} · {}</span></button>",if v.selected.as_deref()==Some(&e.id){"selected"}else{""},e.id,escape(&e.experiment),escape(&e.radar),e.hours.len(),escape(&e.sources.join(", ")),if e.products.is_empty(){"Raw"}else{"Products available"})).collect::<String>()
    };
    el("experiments").set_inner_html(&html);
    let Some(e) = v
        .inventory
        .experiments
        .iter()
        .find(|e| Some(&e.id) == v.selected.as_ref())
    else {
        el("experiment-detail").set_inner_html("");
        return;
    };
    let mut html=format!("<h2>{} · {}</h2><p>Observed hours (UTC): {}</p><div class='experiment-tabs'><button data-product-tab='ion' aria-pressed='true'>Ion-line</button><button data-product-tab='plasma' aria-pressed='false'>Plasma-line</button></div>",escape(&e.experiment),e.date,escape(&e.hours.join(", ")));
    for kind in ["ion", "plasma"] {
        html.push_str(&format!("<div id='products-{kind}' {}><table><thead><tr><th>Analysis</th><th>Status</th><th>Open</th></tr></thead><tbody>",if kind!=v.tab{"hidden"}else{""}));
        let products: Vec<_> = e
            .products
            .iter()
            .filter(|p| p.label.to_lowercase().contains(kind))
            .collect();
        if products.is_empty() {
            html.push_str("<tr><td colspan='3'>No published products</td></tr>");
        }
        for p in products {
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td><a href='{}'>Open {kind}-line</a></td></tr>",
                escape(&p.label),
                if p.complete {
                    "Complete"
                } else {
                    "Processing / partial"
                },
                escape(&p.link)
            ));
        }
        html.push_str("</tbody></table></div>");
    }
    if let Some(c) = &e.calibration {
        html.push_str(&format!("<p><strong>Processing calibration: C = {:.4}</strong> · revision {} · {} of {} points included. Fits retain the calibration revision used.</p>",c.magic_constant,c.revision,c.included,c.total));
    }
    html.push_str(&format!("<p>{}</p><button data-process='{}' {}>Process missing products · 128 cores</button><p class='caption'>Completed products are reused. Jobs run one at a time within the configured core budget.</p>",if e.supported{"Validated processing recipe available."}else{"Processing unavailable: no validated decoder/model recipe configured for this experiment and date."},e.id,if e.supported{""}else{"disabled"}));
    el("experiment-detail").set_inner_html(&html);
}
async fn queue() -> Result<(), JsValue> {
    let jobs: Vec<Job> = json("catalog/queue.json").await?;
    el("queue-list").set_inner_html(&if jobs.is_empty(){"<p>The processing queue is empty.</p>".into()}else{jobs.iter().rev().map(|j|format!("<article class='queue-job'><h3>{} · {}</h3><p><strong>{}</strong> · {} cores · {} / {} stages</p><progress value='{}' max='{}'></progress><p>{}</p><p>{}</p><small>{}</small></article>",j.date,escape(&j.experiment),escape(&j.state),j.cores,j.completed,j.total,j.completed,j.total.max(1),escape(&j.stage),escape(&format!("{}{}",j.message,j.calibration.as_ref().map(|c|format!(" Calibration C={:.4}; revision {}; {} included points.",c.magic_constant,c.revision,c.included)).unwrap_or_default())),escape(&j.id))).collect::<String>()});
    Ok(())
}
async fn post(path: &str) -> Result<(), JsValue> {
    let key = input("processing-key").value();
    if key.trim().is_empty() {
        return Err(error(
            "Enter the server processing key in Compute settings.",
        ));
    }
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    let headers = web_sys::Headers::new()?;
    headers.set("Authorization", &format!("Bearer {}", key.trim()))?;
    init.set_headers(&headers);
    let r: web_sys::Response = JsFuture::from(
        web_sys::window()
            .unwrap()
            .fetch_with_str_and_init(path, &init),
    )
    .await?
    .dyn_into()?;
    if !r.ok() {
        let msg = JsFuture::from(r.text()?)
            .await?
            .as_string()
            .unwrap_or_default();
        return Err(error(msg));
    }
    Ok(())
}
#[wasm_bindgen]
pub async fn start_catalog() -> Result<(), JsValue> {
    let inventory: Inventory = json("catalog/catalog.json").await?;
    let date = inventory
        .experiments
        .iter()
        .find(|e| !e.products.is_empty())
        .or_else(|| inventory.experiments.last())
        .map(|e| e.date.clone())
        .unwrap_or("2022-11-02".into());
    let state = Rc::new(RefCell::new(View {
        month: date[..7].into(),
        date,
        inventory,
        filter: String::new(),
        selected: None,
        tab: "ion".into(),
    }));
    input("month").set_value(&state.borrow().month);
    let months: std::collections::BTreeSet<_> = state
        .borrow()
        .inventory
        .experiments
        .iter()
        .map(|e| e.date[..7].to_owned())
        .collect();
    el("available-months").set_inner_html(&format!(
        "<option value=''>Jump to available month…</option>{}",
        months
            .iter()
            .map(|m| format!("<option>{m}</option>"))
            .collect::<String>()
    ));
    render(&state.borrow());
    queue().await?;
    if web_sys::window().unwrap().location().hash()? == "#queue" {
        el("calendar-view").set_attribute("hidden", "")?;
        el("queue-view").remove_attribute("hidden")?;
    }
    for id in ["month", "filter", "available-months"] {
        let st = state.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let mut v = st.borrow_mut();
            if id == "filter" {
                v.filter = input(id).value()
            } else {
                let value = if id == "month" {
                    input(id).value()
                } else {
                    el(id).dyn_into::<HtmlSelectElement>().unwrap().value()
                };
                if value.len() != 7 {
                    return;
                }
                v.month = value;
                input("month").set_value(&v.month);
            }
            render(&v);
        });
        el(id).add_event_listener_with_callback("change", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    let st = state.clone();
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(target) = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let Ok(Some(button)) = target.closest("button") else {
            return;
        };
        if let Some(date) = button.get_attribute("data-date") {
            let mut v = st.borrow_mut();
            v.date = date;
            v.selected = None;
            render(&v);
        }
        if let Some(id) = button.get_attribute("data-experiment") {
            let mut v = st.borrow_mut();
            v.selected = Some(id);
            v.tab = "ion".into();
            render(&v);
        }
        if let Some(kind) = button.get_attribute("data-product-tab") {
            st.borrow_mut().tab = kind.clone();
            for k in ["ion", "plasma"] {
                let e = el(&format!("products-{k}"));
                if k == kind {
                    let _ = e.remove_attribute("hidden");
                } else {
                    let _ = e.set_attribute("hidden", "");
                }
            }
        }
        if let Some(id) = button.get_attribute("data-process") {
            spawn_local(async move {
                text("action-status", "Submitting processing request…");
                match post(&format!("api/catalog/jobs/{id}")).await {
                    Ok(()) => {
                        text("action-status", "Job queued. See Processing queue.");
                        let _ = queue().await;
                    }
                    Err(e) => text("action-status", &e.as_string().unwrap_or_default()),
                }
            });
        }
        let id = button.id();
        if id == "show-queue" || id == "show-calendar" {
            let show = id == "show-queue";
            for (name, hide) in [("calendar-view", show), ("queue-view", !show)] {
                if hide {
                    let _ = el(name).set_attribute("hidden", "");
                } else {
                    let _ = el(name).remove_attribute("hidden");
                }
            }
        }
        if id == "rescan" {
            spawn_local(async {
                text("action-status", "Requesting archive scan…");
                match post("api/catalog/rescan").await {
                    Ok(()) => text(
                        "action-status",
                        "Archive scan started; inventory will update as it progresses.",
                    ),
                    Err(e) => text("action-status", &e.as_string().unwrap_or_default()),
                }
            });
        }
    });
    doc().add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
    cb.forget();
    let cb = Closure::<dyn FnMut()>::new(move || {
        let st = state.clone();
        spawn_local(async move {
            let _ = queue().await;
            if let Ok(inv) = json::<Inventory>("catalog/catalog.json").await {
                let changed = inv.updated != st.borrow().inventory.updated;
                if changed {
                    st.borrow_mut().inventory = inv;
                    let months: std::collections::BTreeSet<_> = st
                        .borrow()
                        .inventory
                        .experiments
                        .iter()
                        .map(|e| e.date[..7].to_owned())
                        .collect();
                    el("available-months").set_inner_html(&format!(
                        "<option value=''>Jump to available month…</option>{}",
                        months
                            .iter()
                            .map(|m| format!("<option>{m}</option>"))
                            .collect::<String>()
                    ));
                    render(&st.borrow());
                }
            }
        });
    });
    web_sys::window()
        .unwrap()
        .set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            5000,
        )?;
    cb.forget();
    Ok(())
}
