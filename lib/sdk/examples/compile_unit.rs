//! Compile a `.unit` + `.bsi` pair into the runtime unit payload, like
//! `dtmt build` does for package entries.
//!
//! Usage: `cargo run -p sdk --example compile_unit -- <unit> <bsi> <out>`

use sdk::filetype::unit;
use sdk::murmur::{IdString64, Murmur64};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [unit_path, bsi_path, out_path] = args.as_slice() else {
        eprintln!("usage: compile_unit <unit> <bsi> <out>");
        std::process::exit(1);
    };

    let unit_text = std::fs::read_to_string(unit_path)?;
    let bsi = std::fs::read(bsi_path)?;

    // The resource name is the path without extension, as dtmt names bundles.
    let stem = std::path::Path::new(unit_path)
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/");
    let name = IdString64::from(Murmur64::hash(&stem));

    let file = unit::compile(name, &unit_text, &bsi)?;
    let payload = file.variants()[0].data();
    std::fs::write(out_path, payload)?;
    println!("wrote {out_path} ({} bytes)", payload.len());
    Ok(())
}
