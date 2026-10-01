//! Which content-blocker keys does this WebKitGTK accept? Compiles one small filter per
//! feature the DNR translator emits and prints the verdicts. Evidence for what
//! `vsesvit_webext::dnr` may produce on the installed WebKit; nothing here is a test.
//!
//! ```text
//! bash scripts/wsl.sh run -p vsesvit-webext --example dnr_compile
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("dnr_compile runs on Linux only");
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    use std::process::ExitCode;

    use webkit::gio;
    use webkit::glib;

    gtk::init().expect("gtk::init");
    let dir = std::env::temp_dir().join(format!("vsesvit-dnr-compile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let store = webkit::UserContentFilterStore::new(&dir.to_string_lossy());

    let cases: Vec<(&str, String)> = vec![
        ("block + resource types (image, script, fetch, child-document)", r#"[{"trigger":{"url-filter":"x","resource-type":["image","script","fetch","child-document"]},"action":{"type":"block"}}]"#.into()),
        ("resource-type top-document", r#"[{"trigger":{"url-filter":"x","resource-type":["top-document"]},"action":{"type":"block"}}]"#.into()),
        ("resource-type other/ping/media/websocket/font/style-sheet", r#"[{"trigger":{"url-filter":"x","resource-type":["other","ping","media","websocket","font","style-sheet"]},"action":{"type":"block"}}]"#.into()),
        ("ignore-following-rules", r#"[{"trigger":{"url-filter":"x"},"action":{"type":"ignore-following-rules"}},{"trigger":{"url-filter":"x"},"action":{"type":"block"}}]"#.into()),
        ("make-https", r#"[{"trigger":{"url-filter":"x"},"action":{"type":"make-https"}}]"#.into()),
        ("load-type third-party", r#"[{"trigger":{"url-filter":"x","load-type":["third-party"]},"action":{"type":"block"}}]"#.into()),
        ("if-domain / unless-domain", r#"[{"trigger":{"url-filter":"x","if-domain":["*example.com"]},"action":{"type":"block"}},{"trigger":{"url-filter":"y","unless-domain":["*example.org"]},"action":{"type":"block"}}]"#.into()),
        ("if-frame-url", r#"[{"trigger":{"url-filter":"x","if-frame-url":["^[^:]+://+([^:/]+\\.)?a\\.test[:/]"]},"action":{"type":"block"}}]"#.into()),
        ("unless-frame-url", r#"[{"trigger":{"url-filter":"x","unless-frame-url":["^[^:]+://+([^:/]+\\.)?a\\.test[:/]"]},"action":{"type":"block"}}]"#.into()),
        ("if-domain + if-frame-url on one trigger (expected REJECTED: one condition per trigger)", r#"[{"trigger":{"url-filter":"x","if-domain":["*example.com"],"if-frame-url":["^[^:]+://+([^:/]+\\.)?a\\.test[:/]"]},"action":{"type":"block"}}]"#.into()),
        ("if-top-url", r#"[{"trigger":{"url-filter":".*","if-top-url":["^[^:]+://+([^:/]+\\.)?trusted\\.test"]},"action":{"type":"ignore-following-rules"}}]"#.into()),
        ("load-context child-frame", r#"[{"trigger":{"url-filter":"x","load-context":["child-frame"]},"action":{"type":"block"}}]"#.into()),
        ("request-method (one string per rule)", r#"[{"trigger":{"url-filter":"x","request-method":"post"},"action":{"type":"block"}},{"trigger":{"url-filter":"x","request-method":"get"},"action":{"type":"block"}}]"#.into()),
        ("url-filter-is-case-sensitive", r#"[{"trigger":{"url-filter":"X","url-filter-is-case-sensitive":true},"action":{"type":"block"}}]"#.into()),
        ("redirect url", r#"[{"trigger":{"url-filter":"x"},"action":{"type":"redirect","redirect":{"url":"https://example.com/"}}}]"#.into()),
        ("redirect regex-substitution", r#"[{"trigger":{"url-filter":"(x)"},"action":{"type":"redirect","redirect":{"regex-substitution":"https://example.com/$1"}}}]"#.into()),
        ("redirect transform", r#"[{"trigger":{"url-filter":"x"},"action":{"type":"redirect","redirect":{"transform":{"scheme":"https","query-transform":{"remove-parameters":["utm_source"]}}}}}]"#.into()),
        ("modify-headers", r#"[{"trigger":{"url-filter":"x"},"action":{"type":"modify-headers","priority":1,"request-headers":[{"operation":"remove","header":"Cookie"}],"response-headers":[{"operation":"set","header":"X-A","value":"1"}]}}]"#.into()),
        ("regex: domain anchor", r#"[{"trigger":{"url-filter":"^[^:]+://+([^:/]+\\.)?example\\.com\\/ads"},"action":{"type":"block"}}]"#.into()),
        ("regex: separator class", r#"[{"trigger":{"url-filter":"abc[^-.%a-zA-Z0-9_]def"},"action":{"type":"block"}}]"#.into()),
        ("regex: end anchor", r#"[{"trigger":{"url-filter":".*\\/track\\?id=.*$"},"action":{"type":"block"}}]"#.into()),
    ];

    let main_loop = glib::MainLoop::new(None, false);
    let results = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let total = cases.len();
    for (i, (name, json)) in cases.into_iter().enumerate() {
        let (results, main_loop) = (results.clone(), main_loop.clone());
        let bytes = glib::Bytes::from(json.as_bytes());
        store.save(&format!("case{i}"), &bytes, None::<&gio::Cancellable>, move |res| {
            let verdict = match res {
                Ok(_) => "compiles".to_owned(),
                Err(e) => format!("REJECTED: {e}"),
            };
            results.borrow_mut().push((i, name, verdict));
            if results.borrow().len() == total {
                main_loop.quit();
            }
        });
    }
    glib::timeout_add_local_once(std::time::Duration::from_secs(30), {
        let main_loop = main_loop.clone();
        move || main_loop.quit()
    });
    main_loop.run();

    let mut results = results.borrow().clone();
    results.sort_by_key(|(i, _, _)| *i);
    let mut rejected = 0;
    for (_, name, verdict) in &results {
        println!("{name}: {verdict}");
        rejected += usize::from(verdict.starts_with("REJECTED"));
    }
    println!("{} of {total} compiled; {rejected} rejected", results.len() - rejected);
    let _ = std::fs::remove_dir_all(&dir);
    if results.len() == total { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
