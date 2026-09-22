use sdk::murmur::Murmur64;

/// Print the murmur64 hash of each argument, for identifying bundle/resource names.
///
/// Usage: `hash <string>...`
fn main() {
    for s in std::env::args().skip(1) {
        println!("{:016x}  {}", u64::from(Murmur64::hash(s.as_bytes())), s);
    }
}
