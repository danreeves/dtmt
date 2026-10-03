//! Parses the VT2 compiler's `shader_cache.db`: every shader library key and the
//! source record (shader name, defines, compile condition) it was built from.
//!
//! The file opens `{u32 version=3, u32 0, u32 a, u32 b}` then holds
//! length-prefixed strings and their records. Rather than trust a fixed record
//! stride, this walk looks for the known suffixes (`.shader_source`,
//! `.shader_library`) and reads the neighbouring text, which is robust to the
//! binary fields interleaved between them.
//!
//! ```text
//! shader_cache <shader_cache.db> [--keys]
//! ```

use std::error::Error;
use std::fs;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// A length-prefixed printable string starting at `at`, if there is one.
fn string_at(data: &[u8], at: usize) -> Option<(String, usize)> {
    if at + 4 > data.len() {
        return None;
    }
    let len = u32_at(data, at) as usize;
    if !(2..=512).contains(&len) || at + 4 + len > data.len() {
        return None;
    }
    let bytes = &data[at + 4..at + 4 + len];
    if bytes.iter().all(|b| (0x20..0x7f).contains(b)) {
        Some((String::from_utf8_lossy(bytes).to_string(), at + 4 + len))
    } else {
        None
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().ok_or("usage: shader_cache <db>")?;
    let keys_only = args.iter().any(|a| a == "--keys");
    let data = fs::read(path)?;

    let mut at = 24usize;
    let mut keys = 0;
    let mut sources = 0;
    while at < data.len() {
        match string_at(&data, at) {
            Some((text, next)) => {
                if text.ends_with(".shader_source") {
                    sources += 1;
                    if !keys_only {
                        println!("SOURCE {text}");
                    }
                } else if text.ends_with(".shader_library") {
                    keys += 1;
                    println!("{text}");
                } else if !keys_only && !text.is_empty() {
                    // a pass name or a condition
                    println!("      {text}");
                }
                at = next;
            }
            None => at += 1,
        }
    }
    eprintln!("{keys} library keys, {sources} sources");
    Ok(())
}
