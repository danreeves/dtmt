use std::io::Cursor;

use sdk::{BundleDatabase, FromBinary};

/// Dump bundle database entries, optionally for a single bundle hash.
///
/// Usage: `db_dump [<database> [<bundle-hash-hex>]]`
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: db_dump <database> [<hash>]");
    let target = args.next().map(|h| u64::from_str_radix(&h, 16).expect("hash must be hex"));

    let bin = std::fs::read(&path).expect("failed to read database");
    let db = BundleDatabase::from_binary(&mut Cursor::new(bin)).expect("failed to parse database");

    let mut lines = Vec::new();
    for (bundle_hash, files) in db.bundles() {
        let bundle_hash = u64::from(*bundle_hash);
        if let Some(target) = target {
            if bundle_hash != target {
                continue;
            }
        }

        for f in files {
            lines.push(format!(
                "{bundle_hash:016x} name={:?} stream={:?} platform_specific={} unknown={:02x?} file_time={}",
                f.name, f.stream, f.platform_specific, f.unknown, f.file_time
            ));
        }
    }

    lines.sort();
    for line in lines {
        println!("{line}");
    }
}
