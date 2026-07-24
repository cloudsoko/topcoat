//! Desk v1 (WO-009) — the product's face on the hardened kernel.
//!
//! Graduated from the WO-002 proof-harness. The Desk is a pure renderer of
//! (DocType metadata, record JSON) per ADR-004's headless contract, and a
//! CLIENT of the kernel REST surface — every byte of data it shows or writes
//! travels through `frust serve` under the acting user's session token. No
//! root SQL, no direct SurrealDB, no role headers (WO-009 criterion 1).
//!
//! v1 boundaries (standing): list refresh is polling (meta-refresh) until
//! Topcoat ships push; no spreadsheet grids; dependent-field logic would be
//! client-side per ADR-007 — the v1 field set has none, so no runtime
//! signals are used at all. Every interaction is one kernel round-trip.

use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    cookie::{Cookie, Cookies, RouterBuilderCookieExt, cookies},
    router::{
        Form, Router, RouterBuilderDiscoverExt, SeeOther, internal_server_error, layout, page,
        path_param, query_params, redirect, route, see_other,
    },
    view::{component, view},
};

#[tokio::main]
async fn main() {
    // no asset bundle, no client runtime: Desk v1 is fully server-rendered
    // (zero signals/shards — the v1 field set has no dependent fields)
    topcoat::start(Router::builder().cookies().discover().build()).await.unwrap();
}

// ── Kernel client ───────────────────────────────────────────────────────────

mod kernel {
    pub fn base() -> String {
        std::env::var("FRUST_KERNEL").unwrap_or_else(|_| "http://127.0.0.1:8790".into())
    }

    /// One kernel call. Non-2xx comes back as Ok((code, body)) so callers
    /// can render typed errors as user-facing messages.
    pub fn call(token: Option<&str>, route: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        let mut req = agent.post(format!("{}{}", base(), route));
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        match req.send(body.to_string()) {
            Ok(mut r) => {
                let code = r.status().as_u16();
                let parsed = r.body_mut().read_json().unwrap_or(serde_json::json!({}));
                (code, parsed)
            }
            Err(e) => (502, serde_json::json!({ "error": { "kind": "transport", "detail": e.to_string() } })),
        }
    }

    /// Typed kernel errors -> user-facing words. Engine internals stay out
    /// of the DOM (ADR-007 hygiene): machine codes are translated, raw DB
    /// wording is dropped.
    pub fn friendly(code: u16, body: &serde_json::Value) -> String {
        let err = body.get("error").cloned().unwrap_or_default();
        let kind = err.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        let detail = err.get("detail").and_then(|d| d.as_str()).unwrap_or("");
        match kind {
            "hook-rejected" => format!(
                "Rejected by a validation rule: {}",
                err.get("message").and_then(|m| m.as_str()).unwrap_or("the document was not accepted")
            ),
            "identity-unresolved" => "Your identity could not be resolved — contact an administrator.".into(),
            "permission-denied" if code == 401 => "Your session has expired — please log in again.".into(),
            "permission-denied" => "You don't have permission to do that.".into(),
            "field-not-readable" => "You don't have permission to see one of those fields.".into(),
            "unknown-doctype" => "That DocType does not exist.".into(),
            "invalid-value" => format!("Invalid input: {detail}"),
            "write-conflict-exhausted" => "The system is busy — please try again.".into(),
            "db" if detail.contains("FRUST:E_DOCSTATUS:RESURRECTION") => {
                "This document is cancelled; it can never be edited again.".into()
            }
            "db" if detail.contains("FRUST:E_DOCSTATUS") => {
                "That lifecycle transition is not allowed.".into()
            }
            "db" if detail.contains("FRUST:E_IDENTITY_UNRESOLVED") => {
                "Your identity could not be resolved — contact an administrator.".into()
            }
            _ => "Something went wrong on the server.".into(),
        }
    }
}

// ── Session (kernel-owned; the Desk only carries the cookie) ────────────────

#[derive(Clone)]
struct Session {
    token: String,
    user: String,
    role: String,
}

fn session(cx: &Cx) -> Option<Session> {
    let jar = cookies(cx);
    Some(Session {
        token: jar.get("frust_session")?.value().to_string(),
        user: jar.get("frust_user").map(|c| c.value().to_string()).unwrap_or_default(),
        role: jar.get("frust_role").map(|c| c.value().to_string()).unwrap_or_default(),
    })
}

fn require_session(cx: &Cx) -> Result<Session, topcoat::Error> {
    session(cx).ok_or_else(|| redirect("/login").into())
}

fn err500(msg: impl std::fmt::Display) -> topcoat::Error {
    internal_server_error(std::io::Error::other(msg.to_string())).into()
}

// ── Flash messages (one-shot, cookie-carried) ───────────────────────────────
// POST handlers answer 303 See Other (a 307 would replay the POST against a
// GET page); friendly error text rides a flash cookie the layout renders
// once and clears.

fn pct_encode(s: &str) -> String {
    s.bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric() {
                vec![b as char]
            } else {
                format!("%{b:02X}").chars().collect()
            }
        })
        .collect()
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn flash(cx: &Cx, msg: &str) {
    cookies(cx).add(Cookie::build(("frust_flash", pct_encode(msg))).path("/").build());
}

fn take_flash(cx: &Cx) -> Option<String> {
    let jar = cookies(cx);
    let msg = jar.get("frust_flash").map(|c| pct_decode(c.value()));
    if msg.is_some() {
        jar.remove(Cookie::build(("frust_flash", "")).path("/").build());
    }
    msg.filter(|m| !m.is_empty())
}

// ── DocType metadata (from the kernel, at request time) ─────────────────────

#[derive(Debug, Clone, Deserialize)]
struct DocType {
    name: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    submittable: bool,
    #[serde(default)]
    fields: Vec<DocField>,
    #[serde(default)]
    aggregates: Vec<Aggregate>,
}

#[derive(Debug, Clone, Deserialize)]
struct DocField {
    fieldname: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    fieldtype: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    options: Vec<String>,
    /// Editable while docstatus = 1 (the Frappe allow-on-submit convention).
    #[serde(default)]
    allow_on_submit: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct Aggregate {
    kind: String,
    rollup: String,
    #[serde(default)]
    metrics: Vec<MetricSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct MetricSpec {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    field: String,
}

impl DocType {
    fn label_or_name(&self) -> String {
        if self.label.is_empty() { self.name.replace('_', " ") } else { self.label.clone() }
    }
}

impl DocField {
    fn label_or_name(&self) -> String {
        if self.label.is_empty() { self.fieldname.replace('_', " ") } else { self.label.clone() }
    }
}

fn meta_list(s: &Session) -> anyhow::Result<Vec<DocType>> {
    let (code, body) = kernel::call(Some(&s.token), "/meta", &serde_json::json!({}));
    anyhow::ensure!(code == 200, "meta list: {code}");
    Ok(serde_json::from_value(body["doctypes"].clone()).unwrap_or_default())
}

fn meta_one(s: &Session, name: &str) -> anyhow::Result<DocType> {
    let (code, body) = kernel::call(Some(&s.token), &format!("/meta/{name}"), &serde_json::json!({}));
    anyhow::ensure!(code == 200, "meta {name}: {code}");
    Ok(serde_json::from_value(body["doctype"].clone())?)
}

/// Form value -> the REST typed-value form, per metadata. Money is DECIMAL
/// on the wire (never a float), links are records, checks are bools.
fn typed_value(f: &DocField, raw: &str) -> serde_json::Value {
    let raw = raw.trim();
    match f.fieldtype.as_str() {
        "Currency" => serde_json::json!({ "kind": "decimal", "v": if raw.is_empty() { "0" } else { raw } }),
        "Int" => serde_json::json!({ "kind": "int", "v": raw.parse::<i64>().unwrap_or(0) }),
        "Check" => serde_json::json!({ "kind": "bool", "v": raw == "on" || raw == "true" }),
        "Link" => serde_json::json!({ "kind": "record", "v": raw }),
        _ => serde_json::json!(raw),
    }
}

// ── Layout ──────────────────────────────────────────────────────────────────

#[layout("/")]
async fn root_layout(cx: &Cx, slot: Result) -> Result {
    let sess = session(cx);
    let flash_msg = take_flash(cx);
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                <title>"Frust Desk"</title>
            </head>
            <body style="font-family: system-ui, sans-serif; max-width: 72rem; margin: 1.5rem auto; padding: 0 1rem; color: #1a1a1a;">
                <nav style="display: flex; gap: 1rem; align-items: baseline; border-bottom: 2px solid #eee; padding-bottom: 0.6rem;">
                    <a href="/" style="font-weight: 700; text-decoration: none; color: #1a1a1a;">"Frust Desk"</a>
                    if sess.is_some() {
                        <a href="/reports">"Reports"</a>
                    }
                    <span style="flex: 1;"></span>
                    match &sess {
                        Some(s) => {
                            <span style="color: #666;">(&s.user) " · " (&s.role)</span>
                            <a href="/logout">"log out"</a>
                        }
                        None => {
                            <a href="/login">"log in"</a>
                        }
                    }
                </nav>
                if let Some(m) = &flash_msg {
                    <p style="color: #b00020; background: #fdecea; padding: 8px 12px; border-radius: 4px; margin-top: 0.8rem;">(m)</p>
                }
                (slot?)
            </body>
        </html>
    }
}

// ── Login / logout ──────────────────────────────────────────────────────────

#[page("/login")]
async fn login_page(_cx: &Cx) -> Result {
    view! { login_form(message: None) }
}

#[component]
async fn login_form(message: Option<String>) -> Result {
    view! {
        <h1>"Log in"</h1>
        if let Some(m) = &message {
            <p style="color: #b00020; background: #fdecea; padding: 8px 12px; border-radius: 4px;">(m)</p>
        }
        <form method="post" action="/login-submit" style="max-width: 20rem;">
            <label style="display:block; margin: 8px 0 2px; font-weight: 600;">"User"</label>
            <input type="text" name="user" required="required" style="width: 100%;">
            <label style="display:block; margin: 8px 0 2px; font-weight: 600;">"Password"</label>
            <input type="password" name="pass" required="required" style="width: 100%;">
            <button type="submit" style="margin-top: 12px;">"Log in"</button>
        </form>
    }
}

#[derive(Deserialize)]
struct LoginInput {
    user: String,
    pass: String,
}

#[route(POST "/login-submit")]
async fn login_submit(cx: &Cx, Form(input): Form<LoginInput>) -> Result<SeeOther> {
    let (code, body) = kernel::call(None, "/login", &serde_json::json!({ "user": input.user, "pass": input.pass }));
    if code != 200 {
        flash(cx, "Login failed — check your user and password.");
        return Ok(see_other("/login"));
    }
    let token = body["token"].as_str().unwrap_or_default().to_string();
    let user = body["user"].as_str().unwrap_or_default().to_string();
    let role = body["role"].as_str().unwrap_or_default().to_string();
    let jar = cookies(cx);
    jar.add(Cookie::build(("frust_session", token)).path("/").http_only(true).build());
    jar.add(Cookie::build(("frust_user", user)).path("/").build());
    jar.add(Cookie::build(("frust_role", role)).path("/").build());
    Ok(see_other("/"))
}

#[page("/logout")]
async fn logout(cx: &Cx) -> Result {
    if let Some(s) = session(cx) {
        let _ = kernel::call(Some(&s.token), "/logout", &serde_json::json!({}));
    }
    let jar = cookies(cx);
    for name in ["frust_session", "frust_user", "frust_role"] {
        jar.remove(Cookie::build((name, "")).path("/").build());
    }
    Err(redirect("/login").into())
}

// ── Home: the DocType directory ─────────────────────────────────────────────

#[page("/")]
async fn home(cx: &Cx) -> Result {
    let s = require_session(cx)?;
    let doctypes = meta_list(&s).map_err(err500)?;
    let is_manager = s.role == "manager";
    view! {
        <h1>"DocTypes"</h1>
        <table border="0" cellpadding="6" style="border-collapse: collapse; min-width: 30rem;">
            for dt in &doctypes {
                <tr style="border-bottom: 1px solid #eee;">
                    <td style="font-weight: 600;">(dt.label_or_name())</td>
                    <td>
                        if dt.submittable {
                            <small style="color: #666;">"submittable"</small>
                        }
                    </td>
                    <td><a href=(format!("/list/{}", dt.name))>"list"</a></td>
                    <td><a href=(format!("/form/{}", dt.name))>"new"</a></td>
                </tr>
            }
        </table>
        if is_manager {
            <h2 style="margin-top: 2rem;">"Create a DocType"</h2>
            <form method="post" action="/new-doctype">
                <input type="text" name="name" placeholder="doctype name (e.g. purchase_order)" size="30" required="required">
                <label style="margin-left: 8px;">
                    <input type="checkbox" name="submittable"> "submittable"
                </label>
                <button type="submit" style="margin-left: 8px;">"create + sync"</button>
            </form>
            <p><small>"Metadata lands in the kernel, the schema syncs live, and the list/form render on the next request — no restarts."</small></p>
        }
    }
}

#[derive(Deserialize)]
struct NewDoctype {
    name: String,
    #[serde(default)]
    submittable: Option<String>,
}

#[route(POST "/new-doctype")]
async fn new_doctype(cx: &Cx, Form(input): Form<NewDoctype>) -> Result<SeeOther> {
    let Some(s) = session(cx) else { return Ok(see_other("/login")) };
    let name: String =
        input.name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    if name.is_empty() {
        flash(cx, "The DocType needs a name.");
        return Ok(see_other("/"));
    }
    let meta = serde_json::json!({
        "name": name,
        "label": name.replace('_', " "),
        "submittable": input.submittable.is_some(),
        "fields": [
            { "fieldname": "title", "label": "Title", "fieldtype": "Data", "required": true },
            { "fieldname": "total", "label": "Total", "fieldtype": "Currency", "required": true },
            { "fieldname": "notes", "label": "Notes", "fieldtype": "Text", "allow_on_submit": true },
        ]
    });
    let (code, body) = kernel::call(Some(&s.token), "/doctype", &serde_json::json!({ "meta": meta }));
    if code != 200 {
        flash(cx, &kernel::friendly(code, &body));
        return Ok(see_other("/"));
    }
    Ok(see_other(&format!("/form/{name}")))
}

#[component]
async fn error_block(message: String, back: String) -> Result {
    view! {
        <h1>"Can't do that"</h1>
        <p style="color: #b00020; background: #fdecea; padding: 10px 14px; border-radius: 4px;">(&message)</p>
        <p><a href=(&back)>"← back"</a></p>
    }
}

// ── List view: Tier-0 shapes only ───────────────────────────────────────────

#[path_param(error = bad_request("bad doctype name"))]
struct DoctypeName(String);

#[derive(Default)]
struct ListQuery {
    month: Option<String>,
    field: Option<String>,
    value: Option<String>,
    sort: Option<String>,
    start: u64,
}

fn list_query(cx: &Cx) -> ListQuery {
    #[query_params(error = redirect("?"))]
    struct Raw {
        month: Option<String>,
        f: Option<String>,
        v: Option<String>,
        sort: Option<String>,
        start: Option<String>,
    }
    let raw = query_params::<Raw>(cx).ok();
    match raw {
        Some(r) => ListQuery {
            month: r.month.clone().filter(|m| !m.is_empty()),
            field: r.f.clone().filter(|f| !f.is_empty()),
            value: r.v.clone().filter(|v| !v.is_empty()),
            sort: r.sort.clone().filter(|s| !s.is_empty()),
            start: r.start.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0),
        },
        None => ListQuery::default(),
    }
}

const PAGE: u64 = 20;

/// The Tier-0 discipline (ADR-010, WO-007 rule table): period filters are
/// EQUALITY on the stored month field — this UI cannot express a raw date
/// range. Entity filters are equality on Select/Link fields.
#[page("/list/{doctype_name}")]
async fn list_page(cx: &Cx) -> Result {
    let s = require_session(cx)?;
    let name = path_param::<DoctypeName>(cx)?.to_string();
    let dt = meta_one(&s, &name).map_err(err500)?;
    let q = list_query(cx);

    // build the contract filter from Tier-0 shapes only
    let mut clauses = Vec::new();
    let has_month = dt.fields.iter().any(|f| f.fieldname == "month");
    if let (true, Some(m)) = (has_month, &q.month) {
        clauses.push(serde_json::json!({ "path": "month", "op": "eq", "value": m }));
    }
    if let (Some(f), Some(v)) = (&q.field, &q.value) {
        if let Some(field) = dt.fields.iter().find(|df| &df.fieldname == f) {
            let val = typed_value(field, v);
            clauses.push(serde_json::json!({ "path": f, "op": "eq", "value": val }));
        }
    }
    let filter = match clauses.len() {
        0 => serde_json::Value::Null,
        1 => clauses[0].clone(),
        _ => serde_json::json!({ "and": clauses }),
    };
    let (sort_field, sort_dir) = match &q.sort {
        Some(spec) => match spec.split_once(':') {
            Some((f, d)) => (f.to_string(), d.to_string()),
            None => (spec.clone(), "asc".into()),
        },
        None => ("id".to_string(), "asc".to_string()),
    };
    let body = serde_json::json!({
        "filter": filter,
        "order": { "path": sort_field, "dir": sort_dir },
        "limit": PAGE,
        "start": q.start,
    });
    let (code, out) = kernel::call(Some(&s.token), &format!("/read/{name}"), &body);
    if code == 401 {
        return Err(redirect("/login").into());
    }
    if code != 200 {
        return view! { error_block(message: kernel::friendly(code, &out), back: "/".to_string()) };
    }
    let rows = out["rows"].as_array().cloned().unwrap_or_default();

    let cols: Vec<String> = dt.fields.iter().map(|f| f.fieldname.clone()).collect();
    let base_qs = |start: u64| -> String {
        let mut parts = vec![format!("start={start}")];
        if let Some(m) = &q.month { parts.push(format!("month={m}")); }
        if let (Some(f), Some(v)) = (&q.field, &q.value) { parts.push(format!("f={f}&v={v}")); }
        if let Some(so) = &q.sort { parts.push(format!("sort={so}")); }
        format!("?{}", parts.join("&"))
    };
    let months = recent_months();
    let is_manager = s.role == "manager";

    view! {
        // REQ-6.5.2 degradation path: polling, not sockets
        <meta http-equiv="refresh" content="60">
        <h1>(dt.label_or_name()) " — " (rows.len()) " row(s)"</h1>
        <form method="get" style="display: flex; gap: 0.6rem; align-items: baseline; margin: 0.6rem 0; flex-wrap: wrap;">
            if has_month {
                <label>"Month: "
                    <select name="month">
                        <option value="">"(all)"</option>
                        for m in &months {
                            if Some(m) == q.month.as_ref() {
                                <option selected="selected">(m)</option>
                            } else {
                                <option>(m)</option>
                            }
                        }
                    </select>
                </label>
            }
            <label>"Filter: "
                <select name="f">
                    <option value="">"(field)"</option>
                    for f in dt.fields.iter().filter(|f| matches!(f.fieldtype.as_str(), "Select" | "Link" | "Data")) {
                        if Some(&f.fieldname) == q.field.as_ref() {
                            <option value=(&f.fieldname) selected="selected">(f.label_or_name())</option>
                        } else {
                            <option value=(&f.fieldname)>(f.label_or_name())</option>
                        }
                    }
                </select>
                " = "
                <input type="text" name="v" size="14" value=(q.value.clone().unwrap_or_default())>
            </label>
            <button type="submit">"Apply"</button>
            <a href="?">"clear"</a>
        </form>
        <table border="1" cellpadding="6" style="border-collapse: collapse; width: 100%;">
            <tr style="background: #f7f7f7;">
                <th><a href=(format!("/list/{name}{}", base_qs(0)).replace("start=0", "start=0&sort=id:asc"))>"id"</a></th>
                <th>"status"</th>
                if dt.submittable {
                    <th>"docstatus"</th>
                }
                for c in &cols {
                    <th><a href=(format!("/list/{name}?sort={c}:asc"))>(c)</a></th>
                }
            </tr>
            for row in &rows {
                <tr>
                    <td>
                        <a href=(format!(
                            "/doc/{name}/{}",
                            row["id"].as_str().unwrap_or("?").split_once(':').map(|p| p.1).unwrap_or("?")
                        ))>
                            (row["id"].as_str().unwrap_or("?").to_string())
                        </a>
                    </td>
                    <td>(row["status"].as_str().unwrap_or("").to_string())</td>
                    if dt.submittable {
                        <td>(docstatus_badge(row["docstatus"].as_i64().unwrap_or(0)))</td>
                    }
                    for c in &cols {
                        <td>(cell(&row[c]))</td>
                    }
                </tr>
            }
        </table>
        <p>
            if q.start >= PAGE {
                <a href=(format!("/list/{name}{}", base_qs(q.start - PAGE)))>"← prev"</a>
            }
            " "
            if rows.len() as u64 == PAGE {
                <a href=(format!("/list/{name}{}", base_qs(q.start + PAGE)))>"next →"</a>
            }
        </p>
        <p>
            <a href=(format!("/form/{name}"))>"+ new " (dt.label_or_name())</a>
            if is_manager {
                " · " <small style="color:#666;">"click a row for its audit trail"</small>
            }
        </p>
    }
}

fn cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn docstatus_badge(ds: i64) -> String {
    match ds {
        0 => "Draft".into(),
        1 => "Submitted".into(),
        2 => "Cancelled".into(),
        n => format!("? ({n})"),
    }
}

/// The month picker's options: last 18 months. Tier-0 by construction —
/// the UI offers equality buckets, never a range control.
fn recent_months() -> Vec<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86400;
    // civil date from days since epoch (Howard Hinnant's algorithm, ints only)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let mut m = mp + 3;
    if m > 12 {
        y += 1;
        m -= 12;
    }
    let mut out = Vec::new();
    for _ in 0..18 {
        out.push(format!("{y:04}-{m:02}"));
        m -= 1;
        if m == 0 {
            m = 12;
            y -= 1;
        }
    }
    out
}

// ── Form: new record ────────────────────────────────────────────────────────

#[page("/form/{doctype_name}")]
async fn form_page(cx: &Cx) -> Result {
    let s = require_session(cx)?;
    let name = path_param::<DoctypeName>(cx)?.to_string();
    let dt = meta_one(&s, &name).map_err(err500)?;
    view! {
        <h1>"New " (dt.label_or_name())</h1>
        <form method="post" action=(format!("/submit/{}", dt.name))>
            for field in &dt.fields {
                field_row(field: field, value: String::new(), editable: true)
            }
            <button type="submit" style="margin-top: 10px;">"Save draft"</button>
        </form>
    }
}

#[component]
async fn field_row(field: &DocField, value: String, editable: bool) -> Result {
    view! {
        <div style="margin: 10px 0;">
            <label style="display: block; font-weight: 600;">
                (field.label_or_name())
                if field.required {
                    <span style="color: #b00020;">" *"</span>
                }
            </label>
            if !editable {
                <div style="padding: 4px 0; color: #444;">
                    if value.is_empty() { <i style="color:#999;">"—"</i> } else { (&value) }
                </div>
            } else if field.fieldtype == "Select" {
                <select name=(&field.fieldname)>
                    for opt in &field.options {
                        if opt == &value {
                            <option selected="selected">(opt)</option>
                        } else {
                            <option>(opt)</option>
                        }
                    }
                </select>
            } else if field.fieldtype == "Text" {
                <textarea name=(&field.fieldname) rows="3" cols="48">(&value)</textarea>
            } else if field.fieldtype == "Currency" {
                // decimal in, decimal out — money never becomes a float
                <input type="text" inputmode="decimal" name=(&field.fieldname) value=(&value) placeholder="0.00">
            } else if field.fieldtype == "Check" {
                if value == "true" {
                    <input type="checkbox" name=(&field.fieldname) checked="checked">
                } else {
                    <input type="checkbox" name=(&field.fieldname)>
                }
            } else {
                <input type="text" name=(&field.fieldname) value=(&value) size="40">
            }
        </div>
    }
}

#[route(POST "/submit/{doctype_name}")]
async fn submit_new(cx: &Cx, Form(fields): Form<Vec<(String, String)>>) -> Result<SeeOther> {
    let Some(s) = session(cx) else { return Ok(see_other("/login")) };
    let name = path_param::<DoctypeName>(cx)?.to_string();
    let Ok(dt) = meta_one(&s, &name) else {
        flash(cx, "That DocType does not exist.");
        return Ok(see_other("/"));
    };

    let mut doc = serde_json::Map::new();
    for f in &dt.fields {
        let raw = fields.iter().find(|(k, _)| k == &f.fieldname).map(|(_, v)| v.as_str()).unwrap_or("");
        doc.insert(f.fieldname.clone(), typed_value(f, raw));
    }
    let (code, body) = kernel::call(
        Some(&s.token),
        &format!("/write/{name}"),
        &serde_json::json!({ "doc": doc }),
    );
    if code == 401 {
        return Ok(see_other("/login"));
    }
    if code != 200 {
        flash(cx, &kernel::friendly(code, &body));
        return Ok(see_other(&format!("/form/{name}")));
    }
    let id = body["created"]["id"].as_str().unwrap_or("?");
    let key = id.split_once(':').map(|p| p.1).unwrap_or("?");
    Ok(see_other(&format!("/doc/{name}/{key}")))
}

// ── Record view: the docstatus lifecycle as affordances ─────────────────────

#[path_param(error = bad_request("bad record key"))]
struct RecordKey(String);

#[page("/doc/{doctype_name}/{record_key}")]
async fn doc_page(cx: &Cx) -> Result {
    let s = require_session(cx)?;
    let name = path_param::<DoctypeName>(cx)?.to_string();
    let key = path_param::<RecordKey>(cx)?.to_string();
    let dt = meta_one(&s, &name).map_err(err500)?;

    let body = serde_json::json!({
        "filter": { "path": "id", "op": "eq", "value": { "kind": "record", "v": format!("{name}:{key}") } }
    });
    let (code, out) = kernel::call(Some(&s.token), &format!("/read/{name}"), &body);
    if code == 401 {
        return Err(redirect("/login").into());
    }
    if code != 200 {
        return view! { error_block(message: kernel::friendly(code, &out), back: format!("/list/{name}")) };
    }
    let Some(row) = out["rows"].as_array().and_then(|a| a.first()).cloned() else {
        return view! { error_block(
            message: "No such record — or it isn't yours to see.".to_string(),
            back: format!("/list/{name}"),
        ) };
    };

    let ds = row["docstatus"].as_i64().unwrap_or(0);
    // the lifecycle as affordances; the lattice EVENT is the floor beneath
    let (state, editable_all, editable_allowed, can_submit, can_cancel) = if !dt.submittable {
        ("".to_string(), true, true, false, false)
    } else {
        match ds {
            0 => ("Draft".to_string(), true, true, true, false),
            1 => ("Submitted".to_string(), false, true, false, true),
            _ => ("Cancelled".to_string(), false, false, false, false),
        }
    };
    let is_manager = s.role == "manager";

    view! {
        <h1>
            (dt.label_or_name()) " " (&key)
            if dt.submittable {
                <span style=(format!(
                    "margin-left: 0.8rem; font-size: 0.9rem; padding: 2px 10px; border-radius: 10px; background: {};",
                    match ds { 0 => "#fff3cd", 1 => "#d4edda", _ => "#f8d7da" }
                ))>(state)</span>
            }
        </h1>
        if ds == 2 {
            <p style="color: #721c24; background: #f8d7da; padding: 8px 12px; border-radius: 4px;">
                "This document is cancelled. It is permanently read-only."
            </p>
        }
        <form method="post" action=(format!("/save/{name}/{key}"))>
            for field in &dt.fields {
                field_row(
                    field: field,
                    value: cell(&row[&field.fieldname]),
                    editable: editable_all || (editable_allowed && field.allow_on_submit),
                )
            }
            if editable_all || (editable_allowed && dt.fields.iter().any(|f| f.allow_on_submit)) {
                <button type="submit" name="action" value="save" style="margin-top: 10px;">"Save"</button>
            }
            if can_submit {
                <button type="submit" name="action" value="submit" style="margin-left: 8px; font-weight: 700;">"Submit"</button>
            }
            if can_cancel {
                <button type="submit" name="action" value="cancel" style="margin-left: 8px; color: #b00020;">"Cancel document"</button>
            }
        </form>
        <p style="margin-top: 1.2rem;">
            <a href=(format!("/list/{name}"))>"← list"</a>
            if is_manager {
                " · " <a href=(format!("/audit/{name}/{key}"))>"audit trail"</a>
            }
        </p>
    }
}

#[route(POST "/save/{doctype_name}/{record_key}")]
async fn save_doc(cx: &Cx, Form(fields): Form<Vec<(String, String)>>) -> Result<SeeOther> {
    let Some(s) = session(cx) else { return Ok(see_other("/login")) };
    let name = path_param::<DoctypeName>(cx)?.to_string();
    let key = path_param::<RecordKey>(cx)?.to_string();
    let Ok(dt) = meta_one(&s, &name) else {
        flash(cx, "That DocType does not exist.");
        return Ok(see_other("/"));
    };
    let action = fields
        .iter()
        .find(|(k, _)| k == "action")
        .map(|(_, v)| v.as_str())
        .unwrap_or("save");

    let mut doc = serde_json::Map::new();
    match action {
        // lifecycle transitions carry ONLY docstatus — the lattice EVENT
        // under the write is the enforcement, this is just the affordance
        "submit" => {
            doc.insert("docstatus".into(), serde_json::json!({ "kind": "int", "v": 1 }));
        }
        "cancel" => {
            doc.insert("docstatus".into(), serde_json::json!({ "kind": "int", "v": 2 }));
        }
        _ => {
            for f in &dt.fields {
                if let Some((_, raw)) = fields.iter().find(|(k, _)| k == &f.fieldname) {
                    doc.insert(f.fieldname.clone(), typed_value(f, raw));
                }
            }
        }
    }
    let (code, body) = kernel::call(
        Some(&s.token),
        &format!("/write/{name}"),
        &serde_json::json!({ "doc": doc, "record": key }),
    );
    if code == 401 {
        return Ok(see_other("/login"));
    }
    if code != 200 {
        flash(cx, &kernel::friendly(code, &body));
    }
    Ok(see_other(&format!("/doc/{name}/{key}")))
}

// ── Reports: rollup-backed, staleness visible ───────────────────────────────

#[page("/reports")]
async fn reports_index(cx: &Cx) -> Result {
    let s = require_session(cx)?;
    let doctypes = meta_list(&s).map_err(err500)?;
    let mut entries: Vec<(String, String, String)> = Vec::new(); // (rollup, tier, source)
    for dt in &doctypes {
        for a in &dt.aggregates {
            let tier = if a.kind == "counter" { "Tier 1 · exact" } else { "Tier 2 · eventually consistent" };
            entries.push((a.rollup.clone(), tier.to_string(), dt.name.clone()));
        }
    }
    view! {
        <h1>"Reports"</h1>
        if entries.is_empty() {
            <p style="color:#666;">"No aggregates are declared yet. Declare one on a DocType's metadata and it appears here."</p>
        }
        <table border="0" cellpadding="6" style="border-collapse: collapse;">
            for (rollup, tier, source) in &entries {
                <tr style="border-bottom: 1px solid #eee;">
                    <td><a href=(format!("/report/{rollup}"))><b>(rollup.replace('_', " "))</b></a></td>
                    <td><small style="color:#666;">(tier)</small></td>
                    <td><small style="color:#666;">"from " (source)</small></td>
                </tr>
            }
        </table>
    }
}

#[path_param(error = bad_request("bad rollup name"))]
struct RollupName(String);

#[page("/report/{rollup_name}")]
async fn report_page(cx: &Cx) -> Result {
    let s = require_session(cx)?;
    let rollup = path_param::<RollupName>(cx)?.to_string();
    let doctypes = meta_list(&s).map_err(err500)?;
    let agg = doctypes
        .iter()
        .flat_map(|dt| dt.aggregates.iter().map(move |a| (dt, a)))
        .find(|(_, a)| a.rollup == rollup);
    let Some((source_dt, agg)) = agg else {
        return view! { error_block(message: "No such report.".to_string(), back: "/reports".to_string()) };
    };
    let is_tier2 = agg.kind == "worker";
    let metric_names: Vec<String> = agg.metrics.iter().map(|m| m.name.clone()).collect();

    // the rollup is a DocType: read through the same contract as any record
    let body = serde_json::json!({ "order": { "path": "k", "dir": "asc" }, "limit": 200 });
    let (code, out) = kernel::call(Some(&s.token), &format!("/read/{rollup}"), &body);
    if code == 401 {
        return Err(redirect("/login").into());
    }
    if code != 200 {
        return view! { error_block(message: kernel::friendly(code, &out), back: "/reports".to_string()) };
    }
    let rows = out["rows"].as_array().cloned().unwrap_or_default();

    // Tier-2: staleness is data, shown, never hidden (ADR-010)
    let lag = if is_tier2 {
        let (lc, lb) = kernel::call(Some(&s.token), &format!("/lag/{rollup}"), &serde_json::json!({}));
        if lc == 200 { Some(lb) } else { None }
    } else {
        None
    };

    view! {
        <meta http-equiv="refresh" content="60">
        <h1>(rollup.replace('_', " "))</h1>
        <p style="color:#666;">
            if is_tier2 {
                "Tier-2 rollup (worker-maintained, eventually consistent) from "
            } else {
                "Tier-1 rollup (transaction-exact) from "
            }
            <b>(&source_dt.name)</b>
        </p>
        if let Some(l) = &lag {
            <p style=(format!(
                "padding: 6px 12px; border-radius: 4px; background: {};",
                if l["pending"].as_u64().unwrap_or(0) == 0 { "#d4edda" } else { "#fff3cd" }
            ))>
                "Staleness: " <b>(l["pending"].as_u64().unwrap_or(0))</b> " pending change(s)"
                if let Some(at) = l["cursor"]["updated_at"].as_str() {
                    " · cursor advanced " (at)
                }
            </p>
        }
        <table border="1" cellpadding="6" style="border-collapse: collapse; min-width: 32rem;">
            <tr style="background:#f7f7f7;">
                <th>"bucket"</th>
                <th>"count"</th>
                for m in &metric_names {
                    <th>(m)</th>
                }
                if metric_names.is_empty() {
                    <th>"qty"</th>
                    <th>"amount"</th>
                }
            </tr>
            for row in &rows {
                <tr>
                    <td>(row["k"].as_str().unwrap_or("?").to_string())</td>
                    <td style="text-align: right;">(cell(&row["n"]))</td>
                    for m in &metric_names {
                        <td style="text-align: right;">(cell(&row[m]))</td>
                    }
                    if metric_names.is_empty() {
                        <td style="text-align: right;">(cell(&row["qty"]))</td>
                        <td style="text-align: right;">(cell(&row["amount"]))</td>
                    }
                </tr>
            }
        </table>
    }
}

// ── Audit trail (manager surface) ───────────────────────────────────────────

#[page("/audit/{doctype_name}/{record_key}")]
async fn audit_page(cx: &Cx) -> Result {
    let s = require_session(cx)?;
    let name = path_param::<DoctypeName>(cx)?.to_string();
    let key = path_param::<RecordKey>(cx)?.to_string();
    let (code, out) = kernel::call(Some(&s.token), &format!("/audit/{name}/{key}"), &serde_json::json!({}));
    if code == 401 {
        return Err(redirect("/login").into());
    }
    if code != 200 {
        return view! { error_block(message: kernel::friendly(code, &out), back: format!("/doc/{name}/{key}")) };
    }
    let entries = out["entries"].as_array().cloned().unwrap_or_default();
    view! {
        <h1>"Audit trail — " (&name) ":" (&key)</h1>
        <p><small style="color:#666;">
            "Storage-layer history from the table changefeed (REQ-3.2.1) — "
            (entries.len()) " of " (out["total"].as_u64().unwrap_or(0)) " feed entries touch this record."
        </small></p>
        for entry in &entries {
            <pre style="background: #f4f4f4; padding: 8px; overflow-x: auto; border-radius: 4px;">(
                serde_json::to_string_pretty(entry).unwrap_or_default()
            )</pre>
        }
        <p><a href=(format!("/doc/{name}/{key}"))>"← back to the document"</a></p>
    }
}
