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
    processed: String,
    newest: bool,
    page: usize,
    page_size: usize,
    list: bool,
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
fn processed(e: &Entry) -> bool {
    e.products.iter().any(|p| p.complete)
}
fn status(e: &Entry) -> &'static str {
    if processed(e) {
        "Processed"
    } else if e.products.is_empty() {
        "Unprocessed"
    } else {
        "Partial products"
    }
}
fn select(id: &str) -> String {
    el(id).dyn_into::<HtmlSelectElement>().unwrap().value()
}
fn matches(v: &View, e: &Entry) -> bool {
    format!("{} {} {}", e.date, e.experiment, e.radar)
        .to_lowercase()
        .contains(&v.filter.to_lowercase())
        && match v.processed.as_str() {
            "processed" => processed(e),
            "unprocessed" => !processed(e),
            _ => true,
        }
}
fn render(v: &mut View) {
    let mut entries: Vec<_> = v
        .inventory
        .experiments
        .iter()
        .filter(|e| matches(v, e))
        .collect();
    entries.sort_by(|a, b| {
        let dates = if v.newest {
            b.date.cmp(&a.date)
        } else {
            a.date.cmp(&b.date)
        };
        dates
            .then_with(|| a.experiment.cmp(&b.experiment))
            .then_with(|| a.id.cmp(&b.id))
    });
    let count = entries.len();
    let pages = count.div_ceil(v.page_size).max(1);
    v.page = v.page.min(pages - 1);
    let start = v.page * v.page_size;
    text(
        "page-summary",
        &format!(
            "{}–{} of {} experiments · Page {} of {}",
            if count == 0 { 0 } else { start + 1 },
            (start + v.page_size).min(count),
            count,
            v.page + 1,
            pages
        ),
    );
    for (id, disabled) in [
        ("previous-page", v.page == 0),
        ("next-page", v.page + 1 >= pages),
    ] {
        if disabled {
            let _ = el(id).set_attribute("disabled", "");
        } else {
            let _ = el(id).remove_attribute("disabled");
        }
    }
    let rows = entries.iter().skip(start).take(v.page_size).map(|e| format!(
        "<tr class='{}'><td>{}</td><td><button data-experiment='{}'>{}</button></td><td>{}</td><td>{}</td><td>{}</td></tr>",
        if v.selected.as_deref()==Some(&e.id) {"selected-row"} else {""}, escape(&e.date),escape(&e.id),escape(&e.experiment),escape(&e.radar),status(e),e.hours.len()
    )).collect::<String>();
    el("experiment-rows").set_inner_html(&if count == 0 {
        "<tr><td colspan='5'>No experiments match these filters.</td></tr>".into()
    } else {
        rows
    });
    for (id, hide) in [("list-panel", !v.list), ("calendar-panel", v.list)] {
        if hide {
            let _ = el(id).set_attribute("hidden", "");
        } else {
            let _ = el(id).remove_attribute("hidden");
        }
    }
    let _ = el("list-mode").set_attribute("aria-pressed", if v.list { "true" } else { "false" });
    let _ =
        el("calendar-mode").set_attribute("aria-pressed", if v.list { "false" } else { "true" });
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
        let processed = entries.iter().filter(|e| processed(e)).count();
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
        entries.iter().map(|e|format!("<button class='experiment {}' data-experiment='{}'><strong>{}</strong><span>{} · {} hour directories · {} · {}</span></button>",if v.selected.as_deref()==Some(&e.id){"selected"}else{""},e.id,escape(&e.experiment),escape(&e.radar),e.hours.len(),escape(&e.sources.join(", ")),status(e))).collect::<String>()
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
    let mut html=format!("<h2>{} · {}</h2><p>Observed hours (UTC): {}</p><div class='experiment-tabs'><button data-product-tab='ion' aria-pressed='{}'>Ion-line</button><button data-product-tab='plasma' aria-pressed='{}'>Plasma-line</button></div>",escape(&e.experiment),e.date,escape(&e.hours.join(", ")),v.tab=="ion",v.tab=="plasma");
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
        processed: "all".into(),
        newest: true,
        page: 0,
        page_size: 25,
        list: true,
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
    render(&mut state.borrow_mut());
    queue().await?;
    if web_sys::window().unwrap().location().hash()? == "#queue" {
        el("calendar-view").set_attribute("hidden", "")?;
        el("queue-view").remove_attribute("hidden")?;
    }
    for id in [
        "month",
        "filter",
        "available-months",
        "processed-filter",
        "date-order",
        "page-size",
    ] {
        let st = state.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let mut v = st.borrow_mut();
            if id == "filter" {
                v.filter = input(id).value();
                v.page = 0;
                v.selected = None;
            } else if id == "processed-filter" {
                v.processed = select(id);
                v.page = 0;
                v.selected = None;
            } else if id == "date-order" {
                v.newest = select(id) == "newest";
                v.page = 0;
            } else if id == "page-size" {
                v.page_size = select(id).parse().unwrap_or(25);
                v.page = 0;
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
            render(&mut v);
        });
        el(id).add_event_listener_with_callback(
            if id == "filter" { "input" } else { "change" },
            cb.as_ref().unchecked_ref(),
        )?;
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
            render(&mut v);
        }
        if let Some(id) = button.get_attribute("data-experiment") {
            let mut v = st.borrow_mut();
            if let Some(e) = v.inventory.experiments.iter().find(|e| e.id == id) {
                let date = e.date.clone();
                v.month = date[..7].into();
                v.date = date;
                input("month").set_value(&v.month);
            }
            v.selected = Some(id);
            v.tab = "ion".into();
            render(&mut v);
            el("experiment-detail").scroll_into_view();
        }
        if let Some(kind) = button.get_attribute("data-product-tab") {
            let mut v = st.borrow_mut();
            v.tab = kind;
            render(&mut v);
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
        if ["list-mode", "calendar-mode", "previous-page", "next-page"].contains(&id.as_str()) {
            let mut v = st.borrow_mut();
            match id.as_str() {
                "list-mode" => v.list = true,
                "calendar-mode" => v.list = false,
                "previous-page" => v.page = v.page.saturating_sub(1),
                "next-page" => v.page += 1,
                _ => {}
            }
            render(&mut v);
        }
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
                    render(&mut st.borrow_mut());
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
