//! Prints the printable strings of every container in an engine-data file, to
//! see whether the shipped DXIL resource table carries the engine's resource
//! names.
//!
//! ```text
//! container_strings <engine_data text> [max containers]
//! ```

use sdk::filetype::shader;
use sdk::filetype::shader_engine_data::EngineData;

fn main() -> color_eyre::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&args[0])?;
    let engine = match String::from_utf8(bytes.clone()) {
        Ok(text) if text.contains("program ") => EngineData::from_text(&text)?,
        _ => EngineData::from_material(&bytes)?,
    };
    if let Some(path) = args.get(2) {
        // Concatenate every container so the names can be mined out of it.
        let mut all: Vec<u8> = Vec::new();
        for container in &engine.containers {
            all.extend_from_slice(container);
            all.push(0);
        }
        std::fs::write(path, &all)?;
        println!("wrote {} bytes to {path}", all.len());
        return Ok(());
    }
    let max: usize = args
        .get(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(usize::MAX);
    println!("containers: {}", engine.containers.len());
    for (index, container) in engine.containers.iter().enumerate().take(max) {
        println!(
            "=== container {index}: {} bytes, stage {:?}, chunks {:?}",
            container.len(),
            shader::stage_of(container),
            shader::chunks(container)
                .iter()
                .map(|(name, size)| format!("{name}:{size}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut run: Vec<u8> = Vec::new();
        let mut strings: Vec<String> = Vec::new();
        for &byte in container.iter().chain([0u8].iter()) {
            if (0x20..0x7f).contains(&byte) {
                run.push(byte);
            } else {
                if run.len() >= 4 {
                    strings.push(String::from_utf8_lossy(&run).into_owned());
                }
                run.clear();
            }
        }
        for string in strings.iter().take(60) {
            println!("  {string:?}");
        }
        if strings.len() > 60 {
            println!("  ... {} more", strings.len() - 60);
        }
    }
    Ok(())
}
