//! Parses the VT2 compiler's `shader_cache.db` from its bytes, with no assumed
//! field meanings: it walks the length-prefixed strings and reports what stands
//! between them, so the structure can be read off the file rather than assumed.
//!
//! ```text
//! shader_cache <shader_cache.db>
//! ```

use std::error::Error;
use std::fs;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: shader_cache <db>")?;
    let data = fs::read(path)?;
    println!("{} bytes", data.len());

    // Report every length-prefixed printable string, and the byte count between
    // one string's end and the next string's start. If the gaps are constant for
    // a class of string, the record shape is confirmed; if they vary, they are
    // binary fields and the reading must not claim a stride.
    let mut at = 0usize;
    let mut previous_end: Option<(usize, String)> = None;
    let mut printed = 0;
    while at + 4 <= data.len() {
        let len = u32_at(&data, at) as usize;
        let is_string = (2..=512).contains(&len)
            && at + 4 + len <= data.len()
            && data[at + 4..at + 4 + len]
                .iter()
                .all(|b| (0x20..0x7f).contains(b));
        if !is_string {
            at += 1;
            continue;
        }
        let text = String::from_utf8_lossy(&data[at + 4..at + 4 + len]).to_string();
        let gap = previous_end
            .as_ref()
            .map(|(end, _)| at - end)
            .unwrap_or(0);
        println!("  @{at:6} gap={gap:3} len={len:3} {text}");
        previous_end = Some((at + 4 + len, text));
        at += 4 + len;
        printed += 1;
        if printed > 400 {
            println!("  ... (truncated)");
            break;
        }
    }
    Ok(())
}
