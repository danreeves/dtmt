//! Rewrites an engine-data file's derivable program tails to their **split**
//! form: the tail keeps only its device block, so `dtmt build` re-derives the
//! constant-buffer list and the resource lists from each program's compiled
//! container. The group-to-tail assignment and the per-group keying come along
//! (the program-to-group mapping is derived when the file carries no `group`
//! tags).
//!
//! A tail that cannot be derived (its group template disagrees on the descriptor
//! lists and no per-program groups are known) is left carried.
//!
//! ```text
//! split_tails <in.engine_data> [out.engine_data]
//! ```

use sdk::filetype::shader_engine_data::EngineData;

fn main() -> color_eyre::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or_else(|| color_eyre::eyre::eyre!("usage: split_tails <in.engine_data> [out]"))?;
    let output = args.next().unwrap_or_else(|| input.clone());

    let before = std::fs::read_to_string(&input)?;
    let engine = EngineData::from_text(&before)?;
    let text = engine.to_text();
    std::fs::write(&output, &text)?;
    println!(
        "wrote {} bytes to {output} (from {})",
        text.len(),
        before.len()
    );
    Ok(())
}
