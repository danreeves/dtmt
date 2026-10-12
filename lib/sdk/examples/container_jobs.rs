//! Rewrites an engine data file's carried containers as declaration compile
//! jobs.
//!
//! The build can frame a program from the declaration's compiled job (a
//! `job = <n>` line) instead of carrying the container bytes. This compiles
//! `<declaration>`'s jobs against the `.shader_source` libraries under `dir`,
//! matches each program's carried container to the job whose interface it has,
//! and writes the engine data with `job = <n>` per program and no `containers`
//! hex.
//!
//! ```text
//! container_jobs <dir> <declaration.shader_node> <engine_data> <out.engine_data>
//! ```

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::shader;
use sdk::filetype::shader_compile;
use sdk::filetype::shader_engine_data::EngineData;
use sdk::filetype::shader_node::{ShaderNode, STAGES, entry_for, profile_for};
use sdk::filetype::shader_source::ShaderSource;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "shader_source") {
            out.push(path);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err(
            "usage: container_jobs <dir> <declaration.shader_node> <engine_data> <out.engine_data>"
                .into(),
        );
    }
    let dir = Path::new(&args[0]);
    let declaration = Path::new(&args[1]);
    let engine_data_path = Path::new(&args[2]);
    let out = Path::new(&args[3]);

    let node = ShaderNode::from_sjson(&fs::read_to_string(declaration)?)?;
    let mut paths = Vec::new();
    walk(dir, &mut paths)?;
    paths.sort();
    let sources: Vec<ShaderSource> = paths
        .iter()
        .map(|path| Ok(ShaderSource::from_sjson(&fs::read_to_string(path)?)?))
        .collect::<Result<_, Box<dyn Error>>>()?;

    let jobs = node.compile_jobs()?;
    // Each job's vertex container then its pixel one, in job order.
    let mut job_containers: Vec<(shader::Stage, Vec<u8>)> = Vec::new();
    for job in &jobs {
        for stage in STAGES {
            let profile = profile_for(stage).expect("a known stage has a profile");
            let entry = entry_for(profile).expect("a known profile has an entry");
            let source = node.job_source(job, stage, &sources, None);
            let container = shader_compile::compile(&source, profile, entry)?;
            let stage = match stage {
                "vertex" => shader::Stage::Vertex,
                _ => shader::Stage::Pixel,
            };
            job_containers.push((stage, container));
        }
    }
    println!("{} jobs, {} containers compiled", jobs.len(), job_containers.len());

    let mut engine_data = EngineData::from_text(&fs::read_to_string(engine_data_path)?)?;
    let mut matched = 0usize;
    for index in 0..engine_data.programs.len() {
        let stage = engine_data.programs[index].0;
        let carried = engine_data
            .program_containers
            .get(index)
            .copied()
            .flatten()
            .and_then(|container| engine_data.containers.get(container));
        // Match the carried container to a compiled job: the interface *and*
        // the resources must agree (a family's passes share signatures, so the
        // interface alone mispairs them), tie-broken by the closest size and
        // then by the disassembly.
        let candidates: Vec<(usize, &Vec<u8>)> = match carried {
            Some(carried) => job_containers
                .iter()
                .enumerate()
                .filter(|(_, (candidate_stage, candidate))| {
                    *candidate_stage == stage
                        && shader::interface_mismatch(carried, candidate).is_none()
                        && shader::resource_mismatch(carried, candidate).is_none()
                })
                .map(|(at, (_, candidate))| (at / 2, candidate))
                .collect(),
            None => Vec::new(),
        };
        let carried_len = carried.map(Vec::len).unwrap_or(0);
        // Tie-break the closest size first (cheap), then the disassembly.
        let job = if candidates.is_empty() {
            None
        } else {
            let min_size = candidates
                .iter()
                .map(|(_, candidate)| candidate.len().abs_diff(carried_len))
                .min()
                .unwrap_or(0);
            let tied: Vec<&(usize, &Vec<u8>)> = candidates
                .iter()
                .filter(|(_, candidate)| candidate.len().abs_diff(carried_len) == min_size)
                .collect();
            if tied.len() == 1 {
                Some(tied[0].0)
            } else {
                let carried_dxil = carried.and_then(|carried| dxc::disassemble(carried).ok());
                tied.iter()
                    .find(|(_, candidate)| {
                        carried_dxil
                            .as_ref()
                            .is_some_and(|shipped| {
                                dxc::disassemble(candidate).is_ok_and(|run| shipped == &run)
                            })
                    })
                    .or_else(|| tied.first())
                    .map(|(job, _)| *job)
            }
        };
        if candidates.len() > 1 {
            println!(
                "  program {index} ({stage:?}): {} interface+resource matches, carried {carried_len} B, chose job {}",
                candidates.len(),
                job.unwrap_or(0)
            );
        }
        if candidates.len() > 1 {
            println!(
                "  program {index} ({stage:?}): {} interface+resource matches, carried {carried_len} B, chose job {}",
                candidates.len(),
                job.unwrap_or(0)
            );
        }
        match job {
            Some(job) => {
                if let Some(slot) = engine_data.program_jobs.get_mut(index) {
                    *slot = Some(job);
                }
                matched += 1;
            }
            None => println!("  program {index} ({stage:?}): no matching job"),
        }
    }

    engine_data.containers = Vec::new();
    engine_data.program_containers = vec![None; engine_data.programs.len()];
    fs::write(out, engine_data.to_text())?;
    println!(
        "matched {matched}/{} programs -> {}",
        engine_data.programs.len(),
        out.display()
    );
    Ok(())
}
