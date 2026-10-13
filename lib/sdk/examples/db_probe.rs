//! Print what a bundle database actually holds, and resolve names against its
//! file index.
//!
//! Usage: `db_probe <bundle_database.data> [name ...]`

use sdk::murmur::Murmur64;
use sdk::{BundleDatabase, FromBinary as _};
use std::io::Cursor;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let path = std::env::args().nth(1).expect("usage: db_probe <db> [name ...]");
    let bin = std::fs::read(&path)?;
    let mut r = Cursor::new(bin);
    let db = BundleDatabase::from_binary(&mut r)?;

    let bundles = db.bundles();
    let files = db.files();
    let total_files: usize = files.values().map(Vec::len).sum();
    println!("bundles                 {}", bundles.len());
    println!("bundles with a file list {}", files.len());
    println!("file entries            {total_files}");
    let orphan = files.keys().filter(|k| !bundles.contains_key(k)).count();
    println!("file-list keys not in the bundle list {orphan}");
    let files_no_list = bundles
        .iter()
        .filter(|(h, fs)| !fs.is_empty() && !files.contains_key(h))
        .count();
    println!("bundles that name files but have no file list {files_no_list}");

    for name in std::env::args().skip(2) {
        let hash = Murmur64::hash(name.clone());
        let hits: Vec<String> = files
            .iter()
            .filter(|(_, fs)| fs.iter().any(|f| f.name == hash))
            .map(|(b, _)| format!("{:016x}", u64::from(*b)))
            .collect();
        if hits.is_empty() {
            println!("{name} ({hash:016x}) -> not in the file index");
        } else {
            println!("{name} ({hash:016x}) -> {}", hits.join(", "));
        }
    }
    Ok(())
}
