//! The Tier-2 six-verb form bridge (SRS REQ-2.2.3, ADR-001 / ADR-007).
//!
//! A metadata-driven form where every field carries its own client-side
//! state, and a small verb set manipulates that state. These are the verbs a
//! sandboxed user script attaches to Tier-1 hook points (`on_load`,
//! `on_change`, `validate`); here they are exercised by handlers to prove the
//! mechanism. Five of the six run entirely in the browser -- no round-trip --
//! which is only possible because each metadata field gets its OWN signal
//! (the per-field-signal pattern; it retires ADR-001's "must shard-re-render"
//! constraint from the 2026-07-23 prototype).
//!
//! | Verb              | Maps to                        | Round-trip |
//! | ----------------- | ------------------------------ | ---------- |
//! | get-value         | read a field's value signal    | no         |
//! | set-value         | write a field's value signal   | no         |
//! | toggle-visibility | write a field's visible signal | no         |
//! | toggle-read-only  | write a field's readonly signal| no         |
//! | validate          | write a field's error signal   | no         |
//! | call-server       | a #[procedure] (shard/proc)    | YES (once) |
//!
//! Money stays a `Decimal` and is never computed client-side: a value that
//! gets stored is the server's job (ADR-007). The bridge computes only
//! display/visibility/validation in the browser.

use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt},
    router::{Router, RouterBuilderDiscoverExt, page},
    runtime::{Signal, SignalDeclaration, procedure},
    view::view,
};

#[tokio::main]
async fn main() {
    topcoat::start(
        Router::builder()
            .discover()
            .assets(AssetBundle::load().unwrap())
            .build(),
    )
    .await
    .unwrap();
}

/// One field of the DocType's metadata (would come from SurrealDB at runtime).
struct Field {
    name: &'static str,
    label: &'static str,
    kind: &'static str, // "select" | "text"
    options: &'static [&'static str],
    default: &'static str,
}

const FIELDS: &[Field] = &[
    Field {
        name: "customer_type",
        label: "Customer type",
        kind: "select",
        options: &["retail", "wholesale"],
        default: "retail",
    },
    Field {
        name: "quantity",
        label: "Quantity",
        kind: "text",
        options: &[],
        default: "1",
    },
    Field {
        name: "discount_pct",
        label: "Discount %",
        kind: "text",
        options: &[],
        default: "0",
    },
    Field {
        name: "notes",
        label: "Notes",
        kind: "text",
        options: &[],
        default: "",
    },
];

fn index_of(name: &str) -> usize {
    FIELDS
        .iter()
        .position(|f| f.name == name)
        .expect("known field")
}

/// call-server verb: a server function reachable from a client expression.
/// In the Desk this proxies the kernel's `db-read`; here it proves the wire.
#[procedure]
async fn check_stock(item: String) -> Result<String> {
    let units = 40 + item.len(); // stand-in for a real kernel query
    Ok(format!("{item}: {units} units in stock"))
}

#[page("/")]
async fn form() -> Result {
    // The form bridge: one signal per field, per concern. Created from runtime
    // metadata in a loop -- the pattern that makes per-field client state
    // possible at all.
    let value: Vec<Signal<String>> = FIELDS
        .iter()
        .map(|f| Signal::new(f.default.to_string()))
        .collect();
    let visible: Vec<Signal<bool>> = FIELDS
        .iter()
        .map(|f| Signal::new(f.name != "discount_pct"))
        .collect(); // discount hidden for retail
    let readonly: Vec<Signal<bool>> = FIELDS.iter().map(|_| Signal::new(false)).collect();
    let error: Vec<Signal<String>> = FIELDS.iter().map(|_| Signal::new(String::new())).collect();

    let qty = index_of("quantity");
    let disc = index_of("discount_pct");
    let notes = index_of("notes");

    view! {
        <!DOCTYPE html>
        <html>
            <head>
                <title>"Frust form bridge"</title>
                topcoat::runtime::script()
            </head>
            <body style="font-family: system-ui; max-width: 40rem; margin: 2rem auto;">
                <h1>"Order"</h1>

                // announce every bridge signal to the browser runtime
                for s in &value { (SignalDeclaration::new(s)) }
                for s in &visible { (SignalDeclaration::new(s)) }
                for s in &readonly { (SignalDeclaration::new(s)) }
                for s in &error { (SignalDeclaration::new(s)) }

                for i in 0..FIELDS.len() {
                    let field = &FIELDS[i];
                    let val = &value[i];
                    let vis = &visible[i];
                    let ro = &readonly[i];
                    let err = &error[i];

                    // toggle-visibility drives :hidden; the row appears/vanishes
                    <div :hidden=$(!vis.get()) style="margin: 0.6rem 0;">
                        <label style="display:block; font-weight:600;">(field.label)</label>

                        // on_change hook point: the driver field runs a rule
                        if field.name == "customer_type" {
                            let disc_vis = &visible[disc];
                            <select
                                :disabled=$(ro.get())
                                @change=$(|e: topcoat::runtime::Event| {
                                    val.set(e.target.value);
                                    // set-value + get-value + toggle-visibility, all client-side
                                    disc_vis.set(val.get() == "wholesale");
                                })
                            >
                                for opt in field.options {
                                    <option>(opt)</option>
                                }
                            </select>
                        } else if field.name == "quantity" {
                            <input
                                :value=$(val.get())
                                :disabled=$(ro.get())
                                @input=$(|e: topcoat::runtime::Event| {
                                    val.set(e.target.value);
                                    // validate verb: flagged inline, no round-trip
                                    err.set(
                                        if val.get().is_empty() {
                                            "Quantity is required".to_owned()
                                        } else if val.get() == "0" {
                                            "Quantity must be positive".to_owned()
                                        } else {
                                            "".to_owned()
                                        }
                                    );
                                })
                            >
                        } else {
                            <input
                                :value=$(val.get())
                                :disabled=$(ro.get())
                                @input=$(|e: topcoat::runtime::Event| val.set(e.target.value))
                            >
                        }

                        <span style="color:#b00020; font-size:0.85rem;">$(err.get())</span>
                    </div>
                }

                // toggle-read-only verb: lock the notes field
                let notes_ro = &readonly[notes];
                <label style="display:block; margin:1rem 0;">
                    <input type="checkbox" @change=$(|_e| notes_ro.set(!notes_ro.get()))>
                    " Lock notes"
                </label>

                // set-value verb: copy quantity into notes
                let qv = &value[qty];
                let nv = &value[notes];
                <button @click=$(|_e| nv.set(qv.get()))>"Copy quantity to notes"</button>

                // call-server verb: the one round-trip. `stock` is a view
                // signal (local binding) so the async closure captures it
                // cleanly; the item comes from a dedicated signal bound below.
                signal stock = String::new();
                signal item = String::from("widget");
                <div style="margin-top:1rem;">
                    <input :value=$(item.get()) @input=$(|e: topcoat::runtime::Event| item.set(e.target.value))>
                    <button @click=$(async |_e| {
                        let msg = check_stock(item.get()).await;
                        stock.set(msg);
                    })>"Check stock"</button>
                    <p id="stock">$(stock.get())</p>
                </div>
            </body>
        </html>
    }
}
