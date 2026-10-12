//! Resolves texture-category hashes (as printed by a texture body) to their
//! engine string, using the full dictionary.
//!
//! ```text
//! cat_resolve <dictionary.csv> <hash>...
//! ```

use color_eyre::Result;
use sdk::murmur::{Dictionary, HashGroup, Murmur32};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let dict = args.next().expect("usage: cat_resolve <dictionary.csv> <hash>...");
    let bytes = std::fs::read(&dict)?;
    let rt = tokio::runtime::Runtime::new()?;
    let dictionary = rt.block_on(Dictionary::from_csv(&bytes[..]))?;

    let mut resolved = 0;
    let mut total = 0;
    for arg in args {
        let hash = u32::from_str_radix(arg.trim_start_matches("0x"), 16)
            .expect("hash must be hex");
        total += 1;
        match dictionary.lookup_short(Murmur32::from(hash), HashGroup::TextureCategory) {
            Some(name) => {
                resolved += 1;
                println!("{hash:08x}  {name}");
            }
            None => println!("{hash:08x}  UNKNOWN"),
        }
    }
    eprintln!("resolved {resolved}/{total}");
    Ok(())
}
