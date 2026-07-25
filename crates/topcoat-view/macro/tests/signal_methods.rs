use topcoat::{context::Cx, view::view};

fn r(v: topcoat::Result) -> String {
    v.unwrap().render(&Cx::default())
}

/// The typed utility methods on signals (#175) compile in expressions and
/// translate to the matching JavaScript method calls.
#[tokio::test]
async fn signal_utility_methods_translate() {
    let cx = &Cx::default();
    let html = r(view! {
        cx =>
        signal open = false;
        signal count = 0.0;
        signal message = std::string::String::new();

        <button @click=$(|_e| open.toggle())>"toggle"</button>
        <button @click=$(|_e| count.increment())>"+"</button>
        <button @click=$(|_e| count.decrement())>"-"</button>
        <button @click=$(|_e| message.push_str("!"))>"append"</button>
    });

    for call in [".toggle()", ".increment()", ".decrement()", ".push_str("] {
        assert!(html.contains(call), "compiled JS contains {call}: {html}");
    }
}

/// `push_str` accepts both borrowed literals and owned strings (the
/// expression language has no `&` operator, so the argument must deref).
#[tokio::test]
async fn push_str_accepts_owned_strings() {
    let cx = &Cx::default();
    let html = r(view! {
        cx =>
        signal message = std::string::String::new();

        <input @input=$(|e: topcoat::runtime::Event| message.push_str(e.target.value))>
    });

    assert!(html.contains(".push_str("), "{html}");
}
