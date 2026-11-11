//! `grok_probe dump` prints the Grok Bot accessibility tree; `grok_probe send "<text>"` types into
//! Nora's chat once (no queue, no confirmation). For checking the UI Automation path by hand.
#[cfg(windows)]
fn main() {
    use aos_nora::grok::GrokWriter;
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("dump") => match aos_nora::grok_uia::dump_tree() {
            Ok(lines) => lines.iter().for_each(|l| println!("{l}")),
            Err(e) => eprintln!("{e}"),
        },
        Some("send") => {
            let text = args.next().unwrap_or_default();
            let w = aos_nora::grok_uia::UiaWriter::new("Nora");
            println!("locked={} result={:?}", aos_nora::grok_uia::session_locked(), w.send(&text));
        }
        _ => eprintln!("usage: grok_probe dump | send <text>"),
    }
}
#[cfg(not(windows))]
fn main() {}
