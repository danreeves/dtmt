const CHUNK_SIZE: usize = 512 * 1024;

fn u32at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn pad16(o: usize) -> usize {
    let r = o % 16;
    if r == 0 { o } else { o + (16 - r) }
}

fn dump(label: &str, b: &[u8]) {
    let mut o = 0usize;
    let format = u32at(b, o);
    o += 4;
    let field1 = u32at(b, o);
    o += 4;
    let n = u32at(b, o) as usize;
    o += 4;
    for _ in 0..32 {
        let _ = u64at(b, o);
        o += 8;
    }
    for _ in 0..n {
        o += 20; // (u64 type, u64 name, u32 props)
    }
    let nch = u32at(b, o) as usize;
    o += 4;
    let mut sizes = Vec::with_capacity(nch);
    for _ in 0..nch {
        sizes.push(u32at(b, o) as usize);
        o += 4;
    }
    let after_sizes = o;
    o = pad16(o);
    let unpacked = u32at(b, o);
    o += 4;
    let unk2 = u32at(b, o);
    o += 4;
    let data_start = o;

    let raw_chunks = sizes.iter().filter(|s| **s == CHUNK_SIZE).count();
    println!("=== {label}: len={} ===", b.len());
    println!(
        "format={format:#010x} field1={field1:#x} entries={n} chunks={nch} unpacked_size={unpacked} unk2={unk2}"
    );
    println!(
        "after_chunk_sizes={after_sizes} pad={} data_start={data_start}",
        data_start - after_sizes
    );
    println!(
        "chunk sizes: min={:?} max={:?} count==CHUNK_SIZE(raw)={raw_chunks}",
        sizes.iter().min(),
        sizes.iter().max()
    );
    println!("first 8 sizes={:?}", &sizes[..sizes.len().min(8)]);
    println!("last 4 sizes={:?}", &sizes[sizes.len().saturating_sub(4)..]);

    // Walk the chunks to verify the stored layout is consistent.
    let mut p = data_start;
    let mut walked = 0usize;
    let mut ok = true;
    for s in &sizes {
        if p + 4 > b.len() {
            ok = false;
            break;
        }
        let inner = u32at(b, p) as usize;
        p = pad16(p + 4);
        if inner != *s || p + s > b.len() {
            ok = false;
            break;
        }
        p += s;
        walked += 1;
    }
    println!(
        "layout walkable={ok} walked_chunks={walked}/{} end={p} (len={})",
        sizes.len(),
        b.len()
    );
}

fn main() {
    let a = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let b = std::fs::read(std::env::args().nth(2).unwrap()).unwrap();
    dump("VANILLA", &a);
    dump("RESERIALIZED", &b);
}
