use sdk::murmur::HashGroup;
use sdk::{Bundle, Context};

/// Dump every file entry of a bundle, for comparing two bundles.
///
/// Usage: `bundle_dump <bundle>`
fn main() {
    let path = std::env::args().nth(1).expect("usage: bundle_dump <bundle>");
    let bin = std::fs::read(&path).expect("failed to read bundle");
    let ctx = Context::new();
    let bundle = Bundle::from_binary(&ctx, "dump".to_string(), bin).expect("failed to parse bundle");

    for f in bundle.files() {
        let ty = u64::from(f.file_type().hash());
        let name_hash = f.base_name().to_murmur64();
        let resolved = ctx.lookup_hash(name_hash, HashGroup::Filename);
        for v in f.variants() {
            println!(
                "{ty:016x} {name_hash:016x} {} props={:08x} external={} size={} dfn={:?}",
                resolved.display(),
                f.props().bits(),
                v.external(),
                v.data().len(),
                v.data_file_name(),
            );
        }
    }
}
