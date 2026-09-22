use sdk::{BundleDatabase, FromBinary};

/// Summarize a bundle database: how many bundles/files are in each section.
///
/// Usage: `db_stats <database> [<bundle-hash-hex>]`
fn main() {
    let path = std::env::args().nth(1).expect("usage: db_stats <database> [hash]");
    let bin = std::fs::read(&path).expect("failed to read database");
    let db = BundleDatabase::from_binary(&mut std::io::Cursor::new(bin)).expect("failed to parse");

    let stored: Vec<(u64, usize)> = db
        .bundles()
        .iter()
        .map(|(h, v)| (u64::from(*h), v.len()))
        .collect();
    let contents: Vec<(u64, usize)> = db
        .files()
        .iter()
        .map(|(h, v)| (u64::from(*h), v.len()))
        .collect();

    println!(
        "stored_files: {} bundles, {} entries",
        stored.len(),
        stored.iter().map(|x| x.1).sum::<usize>()
    );
    println!(
        "bundle_contents: {} bundles, {} entries",
        contents.len(),
        contents.iter().map(|x| x.1).sum::<usize>()
    );

    if let Some(h) = std::env::args().nth(2) {
        let h = u64::from_str_radix(&h, 16).expect("hash must be hex");
        println!("stored[{h:016x}]  = {:?}", stored.iter().find(|x| x.0 == h));
        println!("contents[{h:016x}] = {:?}", contents.iter().find(|x| x.0 == h));
    }
}
