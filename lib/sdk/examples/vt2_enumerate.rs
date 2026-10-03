//! Enumerates the shader variants a declaration asks the engine to build, and
//! names each one the two ways the engine does:
//!
//! - the **library name** (what `debug_file_index.sjson` lists, what a `.editor`
//!   registry keys): `<shader>:<tokens in declaration order>`, lowercased;
//! - the **query key** (what a section's query id hashes): the full
//!   `<shader>:<context>:<defines sorted>:PLATFORM_<p>:RENDERER_<r>`.
//!
//! The walk is the one the compiler does: a declaration's contexts, each
//! multiplied by its permutation sets, then each selected pass. This is the
//! enumerator the notes describe - the declaration side of the `.editor`
//! registries that `vt2_editor` reads and checks.
//!
//! ```text
//! vt2_enumerate <declaration.shader_node> [library.shader_source | library dir]...
//! ```

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::permutation;
use sdk::filetype::shader_node::ShaderNode;
use sdk::filetype::shader_source::ShaderSource;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "shader_source") {
            out.push(path);
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let declaration = args.next().ok_or("usage: vt2_enumerate <declaration> [sources]...")?;
    let paths: Vec<String> = args.collect();

    let text = fs::read_to_string(&declaration)?;
    let node = ShaderNode::from_sjson(&text)?;

    let mut sources = Vec::new();
    for path in &paths {
        let path = PathBuf::from(path);
        if path.is_dir() {
            let mut found = Vec::new();
            walk(&path, &mut found);
            for entry in found {
                sources.push(ShaderSource::from_sjson(&fs::read_to_string(&entry)?)?);
            }
        } else {
            sources.push(ShaderSource::from_sjson(&fs::read_to_string(&path)?)?);
        }
    }

    let shader = Path::new(&declaration)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let jobs = node.compile_jobs()?;
    println!("{shader}: {} compile jobs", jobs.len());

    let mut seen: Vec<String> = Vec::new();
    for job in &jobs {
        // The engine's own naming uses the pass's macros. For the library name
        // the context is dropped and the tokens keep their order.
        let tokens: Vec<&str> = job.macros.iter().map(|m| m.name.as_str()).collect();
        let library = if tokens.is_empty() {
            shader.to_lowercase()
        } else {
            format!("{}:{}", shader.to_lowercase(), tokens.join(":").to_lowercase())
        };
        let query = permutation::key(&shader, &job.context, &tokens, "WIN32", "D3D12");
        let id = permutation::id(&query);
        println!(
            "  {:14} p{} {:24} macros[{}] library={library} id={id:08X}",
            job.context,
            job.permutation,
            job.code_block,
            tokens.join(",")
        );
        seen.push(library);
    }
    println!("\n{} library keys:", seen.len());
    Ok(())
}
