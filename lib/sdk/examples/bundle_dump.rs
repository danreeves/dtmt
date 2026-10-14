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
            // A variant indexes the bundle's `PropertyArray` (issue #3); when that
            // entry is set, it names the variant - a language, typically.
            let property = v.property() as usize;
            let property_name = bundle
                .properties()
                .get(property)
                .filter(|id| u64::from(**id) != 0)
                .map(|id| ctx.lookup_hash(*id, HashGroup::Other).display().to_string())
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{ty:016x} {name_hash:016x} {} props={:08x} external={} size={} dfn={:?} dfn_len={} variant={property} ({property_name})",
                resolved.display(),
                f.props().bits(),
                v.external(),
                v.data().len(),
                v.data_file_name(),
                v.data_file_name_len(),
            );
        }
    }
}
