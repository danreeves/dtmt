use sdk::{Bundle, BundleFile, BundleFileType, BundleFileVariant, Context};

/// Rebuild a boot bundle from a vanilla copy, optionally adding DTMM's boot
/// patch files, to bisect which one crashes the game.
///
/// Usage: `boot_variant <vanilla-bundle> <out> <none|lua|package|both> [lua-file] [package-file]`
fn main() {
    let mut args = std::env::args().skip(1);
    let vanilla = args.next().expect("vanilla bundle path");
    let out = args.next().expect("output path");
    let mode = args.next().expect("mode");
    let lua_path = args.next();
    let pkg_path = args.next();

    let ctx = Context::new();
    let bin = std::fs::read(&vanilla).expect("failed to read vanilla bundle");
    let mut bundle =
        Bundle::from_binary(&ctx, "packages/boot".to_string(), bin).expect("failed to parse");

    if mode == "lua" || mode == "both" {
        let data = std::fs::read(lua_path.expect("lua path")).expect("failed to read lua");
        let mut variant = BundleFileVariant::new();
        variant.set_data(data);
        let mut f = BundleFile::new(0x3F9FC8B1B37DD60Au64, BundleFileType::Lua);
        f.add_variant(variant);
        bundle.add_file(f);
    }

    if mode == "package" || mode == "both" {
        let data = std::fs::read(pkg_path.expect("package path")).expect("failed to read package");
        let mut variant = BundleFileVariant::new();
        variant.set_data(data);
        let mut f = BundleFile::new(0x50740E22C0741FE0u64, BundleFileType::Package);
        f.add_variant(variant);
        bundle.add_file(f);
    }

    let bin = bundle.to_binary().expect("failed to serialize");
    std::fs::write(&out, &bin).expect("failed to write");
    println!(
        "wrote {out} ({} bytes, {} files)",
        bin.len(),
        bundle.files().len()
    );
}
