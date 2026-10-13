//! Dump the raw data of every `Slug` entry in a bundle, for comparing a shipped
//! Darktide font against the Slug library's demo files.
//!
//! Usage: `slug_probe <bundle> <out dir>`

use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let bundle_path = std::env::args().nth(1).expect("usage: slug_probe <bundle> <out dir>");
    let out = std::env::args().nth(2).expect("usage: slug_probe <bundle> <out dir>");
    std::fs::create_dir_all(&out)?;

    let ctx = Context::new();
    let bytes = std::fs::read(&bundle_path)?;
    let name = std::path::Path::new(&bundle_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let bundle = Bundle::from_binary(&ctx, name, &bytes[..])?;

    for file in bundle.files() {
        if file.file_type() != BundleFileType::Slug {
            continue;
        }
        for (index, variant) in file.variants().iter().enumerate() {
            let data = variant.data();
            let path = std::path::Path::new(&out).join(format!("{:016X}_{index}.slug", file.base_name().to_murmur64()));
            std::fs::write(&path, data)?;
            let head: String = data.iter().take(64).map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
            let asc: String = data
                .iter()
                .take(32)
                .map(|b| if (0x20..0x7f).contains(b) { *b as char } else { '.' })
                .collect();
            println!(
                "{:016X} variant {index}: {} bytes\n  head {head}\n  asc  {asc}",
                file.base_name().to_murmur64(),
                data.len()
            );
        }
    }
    Ok(())
}
