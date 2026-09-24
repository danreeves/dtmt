//! Rewrites the constant buffer sizes in every program tail of a family preset.
//!
//! When a mod's shader declares a constant buffer that is larger (or smaller)
//! than the family's shipped one, the engine still binds the size the preset's
//! tails mention, so those have to be patched to match. This tool takes the
//! preset in place and a list of `name-hash=size` assignments, where the hash is
//! the murmur32 of the cbuffer's name (see `dtmt murmur hash <name> --half`).
//!
//! ```text
//! patch_tails <preset> <name-hash=bytes>...
//! ```
//!
//! For example, growing `c_per_object` (B5639618) from 240 to 256 bytes:
//!
//! ```text
//! patch_tails materials/mods/example/ui.preset B5639618=256
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use sdk::filetype::shader::Tail;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((path, assignments)) = args.split_first() else {
        eprintln!("usage: patch_tails <preset> <name-hash=bytes>...");
        std::process::exit(1);
    };

    let mut sizes: HashMap<u32, u32> = HashMap::new();
    for assignment in assignments {
        let (hash, size) = assignment
            .split_once('=')
            .ok_or_else(|| format!("'{assignment}' is not a name-hash=bytes pair"))?;
        let hash = u32::from_str_radix(hash.trim_start_matches("0x"), 16)?;
        sizes.insert(hash, size.parse()?);
    }

    let preset_path = PathBuf::from(path);
    let text = fs::read_to_string(&preset_path)?;
    let mut out = String::with_capacity(text.len() + 1024);
    let mut patched = 0usize;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("program ") {
            let Some((stage, hex)) = rest.split_once(' ') else {
                out.push_str(line);
                out.push('\n');
                continue;
            };
            let bytes = from_hex(hex)?;
            let Some(mut tail) = Tail::parse(&bytes) else {
                eprintln!("warning: {stage} tail does not parse, left alone");
                out.push_str(line);
                out.push('\n');
                continue;
            };
            for entry in &mut tail.cbuffers {
                if let Some(&size) = sizes.get(&entry.name_hash()) {
                    let old = entry.size();
                    entry.words[2] = size;
                    patched += 1;
                    println!("{stage}: {:08X} {old} -> {size}", entry.name_hash());
                }
            }
            out.push_str(&format!("program {stage} {}\n", to_hex(&tail.bytes())));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    fs::write(&preset_path, out)?;
    println!(
        "patched {patched} constant buffer entr(ies) in {}",
        preset_path.display()
    );
    Ok(())
}

fn from_hex(hex: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if hex.len() % 2 != 0 {
        return Err("hex string has an odd length".into());
    }
    (0..hex.len() / 2)
        .map(|i| Ok(u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}
