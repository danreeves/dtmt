//! Checks the device preamble against the `shader_block` model: does a shipped
//! preamble parse as a `BlockTemplate`, and what do its parts say?
//!
//! ```text
//! preamble_check <engine_data text>
//! ```

use std::fs;

use sdk::filetype::shader_block::BlockTemplate;
use sdk::filetype::shader_engine_data::EngineData;

fn main() -> color_eyre::Result<()> {
    let path = std::env::args().nth(1).expect("usage: preamble_check <engine_data>");
    let text = fs::read_to_string(&path)?;
    let engine = EngineData::from_text(&text)?;
    let preamble = &engine.device_preamble;
    println!("device preamble: {} bytes", preamble.len());
    println!("first 32 bytes: {:02X?}", &preamble[..32.min(preamble.len())]);
    if std::env::args().any(|arg| arg == "--hex") {
        let hex: String = preamble.iter().map(|byte| format!("{byte:02x}")).collect();
        println!("HEX {hex}");
    }

    match BlockTemplate::from_preamble(preamble) {
        Ok(template) => {
            println!("parses as a BlockTemplate:");
            println!("  groups   = {}", template.groups());
            println!("  cbuffers = {}", template.cbuffers());
            println!("  records  = {}", template.records().len());
            let names: Vec<String> = template
                .channel_names()
                .iter()
                .map(|hash| format!("{hash:08X}"))
                .collect();
            println!("  channels = {}", names.join(" "));
            println!("  stream   = {} bytes", template.stream().len());
        }
        Err(err) => println!("does NOT parse: {err:#}"),
    }

    Ok(())
}
