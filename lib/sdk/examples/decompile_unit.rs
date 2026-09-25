//! Decompile a compiled unit payload into a `.unit` + `.bsi` authoring pair.
//!
//! Usage: `cargo run -p sdk --example decompile_unit -- <payload> <out dir>`

use sdk::filetype::unit;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [payload_path, out_dir] = args.as_slice() else {
        eprintln!("usage: decompile_unit <payload> <out dir>");
        std::process::exit(1);
    };

    let payload = std::fs::read(payload_path)?;
    let (unit_text, bsi_text) = unit::decompile(&payload)?;

    let stem = std::path::Path::new(payload_path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "unit".to_string());
    let out = std::path::Path::new(out_dir);
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join(format!("{stem}.unit")), unit_text)?;
    std::fs::write(out.join(format!("{stem}.bsi")), bsi_text)?;
    println!("decompiled {payload_path} into {}", out.display());
    Ok(())
}
