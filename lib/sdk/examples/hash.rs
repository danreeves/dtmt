use sdk::murmur::{Murmur32, Murmur64};

/// Print the murmur64 and murmur32 hashes of each argument, for identifying
/// bundle/resource names (64 bit) and shader/material names (32 bit).
///
/// Usage: `hash <string>...`
fn main() {
    for s in std::env::args().skip(1) {
        println!(
            "{:016x}  {:08x}  {}",
            u64::from(Murmur64::hash(s.as_bytes())),
            u32::from(Murmur32::hash(s.as_bytes())),
            s
        );
    }
}
