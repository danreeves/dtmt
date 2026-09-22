use sdk::{Context, decompress};

fn hexdump(b: &[u8], base: usize) {
    for (i, chunk) in b.chunks(16).enumerate() {
        let off = base + i * 16;
        let hex: Vec<String> = chunk.iter().map(|x| format!("{x:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&x| if (32..127).contains(&x) { x as char } else { '.' })
            .collect();
        println!("{off:08x}  {:<47}  {ascii}", hex.join(" "));
    }
}

fn main() {
    let a_path = std::env::args().nth(1).unwrap();
    let b_path = std::env::args().nth(2).unwrap();
    let start: usize = std::env::args()
        .nth(3)
        .map(|s| s.parse().unwrap())
        .unwrap_or(0);

    let ctx = Context::new();
    let a = decompress(&ctx, std::fs::read(&a_path).unwrap()).unwrap();
    let b = decompress(&ctx, std::fs::read(&b_path).unwrap()).unwrap();

    println!("decompressed: A={} B={}", a.len(), b.len());
    let a = &a[start..];
    let b = &b[start..];
    let n = a.len().min(b.len());
    let mut first = None;
    let mut count = 0usize;
    for i in 0..n {
        if a[i] != b[i] {
            count += 1;
            if first.is_none() {
                first = Some(i);
            }
        }
    }
    println!(
        "compared {n} bytes from {start}; differing={count}; first_diff={:?} (abs {})",
        first,
        first.map(|f| f + start).unwrap_or(0)
    );
    if let Some(f) = first {
        let s = f.saturating_sub(48);
        let e = (f + 96).min(n);
        println!("--- A ---");
        hexdump(&a[s..e], start + s);
        println!("--- B ---");
        hexdump(&b[s..e], start + s);
    }
}
