//! Probe the `slug_album` resources against the existing `slug` reader.
//!
//! A `slug_album` opens with the same `guls` magic, version and four-character kind
//! a `slug` does, so the question is whether it is *the same container* with a
//! different kind - in which case `filetype::slug` already reads it - or something
//! else. This prints, for every `slug` and `slug_album` in the install, its kind, its
//! four records and what the reader made of it.
//!
//! Usage: `slug_album_probe <bundle dir>`

use sdk::filetype::slug::SlugFont;
use sdk::{Bundle, BundleFileType, Context};

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn tag(b: &[u8], at: usize) -> String {
    // The 4CC is stored little-endian, so "COMP" reads back as "PMOC".
    b[at..at + 4].iter().rev().map(|c| *c as char).collect()
}

fn dump(data: &[u8]) {
    for row in (0..data.len().min(1024)).step_by(16) {
        let end = (row + 16).min(data.len());
        let bytes = &data[row..end];
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let text: String = bytes
            .iter()
            .map(|b| {
                if (0x20..0x7f).contains(b) {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  {row:04x}  {:<47}  |{text}|", hex.join(" "));
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: slug_album_probe <bundle dir>");

    let ctx = Context::new();
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&arg)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n.len() == 16 && n.chars().all(|c| c.is_ascii_hexdigit())
                })
        })
        .collect();
    entries.sort();

    let mut slugs = 0usize;
    let mut albums = 0usize;
    let mut album_reads = 0usize;
    // The first album is hexdumped whole: its `COMP` regions are the payload the
    // reader does not touch, and inspecting them needs no bundle lookup.
    let mut dumped_an_album = false;

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else {
            continue;
        };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        for file in bundle.files() {
            let kind_of_thing = match file.file_type() {
                BundleFileType::Slug => "slug",
                BundleFileType::SlugAlbum => "slug_album",
                _ => continue,
            };
            let data = file.variants()[0].data();
            if kind_of_thing == "slug" {
                slugs += 1;
            } else {
                albums += 1;
            }
            if kind_of_thing == "slug" {
                continue;
            }
            let kind = String::from_utf8_lossy(&data[8..12])
                .trim_end_matches('\0')
                .to_string();
            let mut records = String::new();
            for i in 0..4 {
                let at = 0x20 + i * 12;
                if at + 12 > data.len() {
                    break;
                }
                records.push_str(&format!(
                    "{}={}/{ } ",
                    tag(data, at),
                    u32_at(data, at + 4),
                    u32_at(data, at + 8)
                ));
            }
            if !dumped_an_album {
                dumped_an_album = true;
                println!("whole first album ({} bytes):", data.len());
                dump(data);
            }
            let read = SlugFont::parse(data);
            match read {
                Ok(font) => {
                    album_reads += 1;
                    println!(
                        "      reader records: {}",
                        font.records
                            .iter()
                            .map(|r| format!("{}={}/{}", r.tag, r.base, r.count))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    println!(
                        "{}  {} B  kind={kind:4}  {records} -> parsed: version {:#06x}, kerning {}, glyphs {}, markers {}, table {:?}",
                        file.base_name().display(),
                        data.len(),
                        font.version,
                        font.kerning.len(),
                        font.glyphs.len(),
                        font.markers.len(),
                        font.glyph_table,
                    );
                }
                Err(e) => println!(
                    "{}  {} B  kind={kind:4}  {records} -> REJECTED: {e}",
                    file.base_name().display(),
                    data.len()
                ),
            }
        }
    }

    println!("slug: {slugs}   slug_album: {albums}   albums the slug reader accepts: {album_reads}");
    Ok(())
}
