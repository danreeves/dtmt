//! Rebuilds each group's `mid` with [`derived_mid`] and compares it to the
//! shipped bytes.
//!
//! The inputs are the shipped section's own: the cbuffer sizes from the `W1`
//! chain, the material's channel rows (the ones named `bca`/`orm`/`nm`), the
//! packed copies from the last section, and `x_end` from the first header's
//! `W2`. This checks the *assembly* - the header interleave, the field
//! placement and `W2` - independently of where the inputs come from.
//!
//! ```text
//! mid_derive <engine_data>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::group_data::{mid_bytes, mid_sections};
use sdk::filetype::shader_engine_data::EngineData;
use sdk::filetype::shader_engine_interface::{channel_records, derived_mid};

/// The material's channels, whose rows are the ones before the engine's fields.
const CHANNELS_NAMED: [u32; 3] = [0x2FAD_CC8C, 0x8655_079C, 0xB45E_95F3];

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: mid_derive <engine_data>")?;
    let engine_data = EngineData::from_text(&fs::read_to_string(&path)?)?;
    let template = engine_data
        .group_template
        .as_ref()
        .ok_or("no template in the file")?;

    for (index, parts) in template.groups.iter().enumerate() {
        let Some(sections) = mid_sections(&parts.mid) else {
            println!("group {index}: the mid does not walk");
            continue;
        };

        // The cbuffer sizes are the sections' own `W1`s (the hashes only matter
        // to `packed_copies`, which this uses the shipped copies for).
        let cbuffers: Vec<(u32, u32)> = sections.iter().map(|section| (section.w1, 0)).collect();
        let x_end = sections.first().map_or(0, |section| section.w2);
        let copies = sections
            .last()
            .map(|section| section.records.clone())
            .unwrap_or_default();
        let channels: Vec<u8> = sections
            .first()
            .map(|section| {
                // The group's channels, in the order its records carry them.
                let mut named: Vec<u32> = Vec::new();
                for row in section.records.chunks_exact(20) {
                    let hash = u32::from_le_bytes(row[8..12].try_into().unwrap());
                    if CHANNELS_NAMED.contains(&hash) && !named.contains(&hash) {
                        named.push(hash);
                    }
                }
                // ... built from the engine's channel table, not copied.
                let built = channel_records(&named);
                for row in built.chunks_exact(20) {
                    let hash = u32::from_le_bytes(row[8..12].try_into().unwrap());
                    assert!(
                        named.contains(&hash),
                        "a channel row names a channel that is not in the group"
                    );
                }
                built
            })
            .unwrap_or_default();

        let built = derived_mid(&cbuffers, &channels, &copies, x_end);
        if built == parts.mid {
            println!("group {index}: identical ({} B)", built.len());
        } else {
            println!(
                "group {index}: shipped {} B, built {} B",
                parts.mid.len(),
                built.len()
            );
            let common = built.len().min(parts.mid.len());
            if let Some(at) = (0..common).find(|i| built[*i] != parts.mid[*i]) {
                let from = at.saturating_sub(8);
                let to = (at + 16).min(common);
                println!(
                    "  first diff at {at}: shipped {:02x?}",
                    &parts.mid[from..to]
                );
                println!("                   built   {:02x?}", &built[from..to]);
            }
        }
        // `mid_bytes` is the exact inverse of `mid_sections`.
        debug_assert_eq!(mid_bytes(&sections), parts.mid);
    }
    Ok(())
}
