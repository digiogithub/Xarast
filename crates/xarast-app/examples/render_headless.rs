//! Render a document to a PNG with no window, no GPU and no compositor.
//!
//! ```text
//! cargo run -p xarast-app --example render_headless -- in.xar out.png [WIDTHxHEIGHT]
//! ```
//!
//! This is the headless path `xarast-cli` wires a subcommand onto; it
//! lives here as an example so that the API is exercised even before the
//! command line grows one.

use std::path::Path;

use xarast_app::{DeviceSize, DocumentId, HeadlessOptions, Session, headless};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("usage: render_headless IN OUT [WxH]")?;
    let output = args.next().ok_or("usage: render_headless IN OUT [WxH]")?;
    let size = match args.next() {
        Some(s) => {
            let (w, h) = s.split_once('x').ok_or("size must be WIDTHxHEIGHT")?;
            DeviceSize::new(w.parse()?, h.parse()?)
        }
        None => DeviceSize::new(1024, 768),
    };

    let session = Session::open(DocumentId(1), Path::new(&input))?;
    let opts = HeadlessOptions {
        size,
        ..HeadlessOptions::default()
    };
    let out = headless::render_to_png(&session, &opts, Path::new(&output))?;
    println!(
        "{output}: {}x{} px, {} commands, {} µs",
        size.width,
        size.height,
        out.commands,
        out.timings.total_us()
    );
    Ok(())
}
