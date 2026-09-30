use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, Report};
use dtmt_shared::ModConfig;
use futures::StreamExt;
use futures::future::try_join_all;
use path_slash::PathExt;
use sdk::filetype::material::{self, ShaderOverrides};
use sdk::filetype::package::Package;
use sdk::filetype::shader::Stage;
use sdk::filetype::shader_compile;
use sdk::filetype::shader_graph::{Evaluation, Graph, NodeDef};
use sdk::filetype::shader_node::{ShaderNode, STAGES, StageResources, entry_for, profile_for};
use sdk::filetype::shader_engine_data::EngineData;
use sdk::filetype::shader_source::ShaderSource;
use sdk::murmur::IdString64;
use sdk::{Bundle, BundleFile, BundleFileType};
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

const PROJECT_CONFIG_NAME: &str = "dtmt.cfg";
/// Marker file written by DTMM while a mod deployment is active.
const DEPLOYMENT_MARKER: &str = "dtmm-deployment.sjson";

type FileIndexMap = HashMap<String, HashSet<String>>;

/// Path of the `.bak` sibling that keeps the original of a deployed file.
fn backup_path_for(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

/// Backs up `path` before it gets overwritten, unless a backup already exists.
///
/// The backup is never replaced, so however many times a file is deployed, the
/// original game file stays recoverable. Files that do not exist yet need no
/// backup (a reset can simply delete them again).
async fn backup_file(path: &Path) -> Result<bool> {
    let backup = backup_path_for(path);

    if fs::try_exists(&backup).await? {
        return Ok(true);
    }

    if !fs::try_exists(path).await? {
        return Ok(false);
    }

    fs::copy(path, &backup).await.wrap_err_with(|| {
        format!(
            "Failed to back up '{}' to '{}' before overwriting it",
            path.display(),
            backup.display()
        )
    })?;

    tracing::info!("Backed up '{}' to '{}'", path.display(), backup.display());
    Ok(true)
}

/// Writes a deployed file, backing up the previous contents first.
async fn write_deployed_file(path: &Path, data: &[u8]) -> Result<()> {
    backup_file(path).await?;

    fs::write(path, data)
        .await
        .wrap_err_with(|| format!("Failed to write '{}'", path.display()))
}

pub(crate) fn command_definition() -> Command {
    Command::new("build")
        .about("Build a project")
        .arg(
            Arg::new("directory")
                .required(false)
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The path to the project to build. \
                        If omitted, dtmt will search from the current working directory upward.",
                ),
        )
        .arg(
            Arg::new("out")
                .long("out")
                .short('o')
                .default_value("out")
                .value_parser(value_parser!(PathBuf))
                .help("The directory to write output files to."),
        )
        .arg(
            Arg::new("deploy")
                .long("deploy")
                .short('d')
                .value_parser(value_parser!(PathBuf))
                .help(
                    "If the path to the game (without the trailing '/bundle') is specified, \
                        deploy the newly built bundles. \
                        This will not adjust the bundle database or package files, so if files are \
                        added or removed, you will have to import into DTMM and re-deploy there.",
                ),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help(
                    "Deploy even when a mod deployment is already active. \
                        This can overwrite files that were already modified, so prefer \
                        running `dtmm --reset` first.",
                ),
        )
}

/// Try to find a `dtmt.cfg` in the given directory or traverse up the parents.
#[tracing::instrument]
async fn find_project_config(dir: Option<PathBuf>) -> Result<ModConfig> {
    let (path, mut file) = if let Some(path) = dir {
        let file = File::open(&path.join(PROJECT_CONFIG_NAME))
            .await
            .wrap_err_with(|| format!("Failed to open file: {}", path.display()))
            .with_suggestion(|| {
                format!(
                    "Make sure the file at '{}' exists and is readable",
                    path.display()
                )
            })?;
        (path, file)
    } else {
        let mut dir = std::env::current_dir()?;
        loop {
            let path = dir.join(PROJECT_CONFIG_NAME);
            match File::open(&path).await {
                Ok(file) => break (dir, file),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if let Some(parent) = dir.parent() {
                        // TODO: Re-write with recursion to avoid allocating the `PathBuf`.
                        dir = parent.to_path_buf();
                    } else {
                        eyre::bail!("Could not find project root");
                    }
                }
                Err(err) => {
                    let err = Report::new(err)
                        .wrap_err(format!("Failed to open file: {}", path.display()));
                    return Err(err);
                }
            }
        }
    };

    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .await
        .wrap_err("Invalid UTF-8")?;

    let mut cfg: ModConfig =
        serde_sjson::from_str(&buf).wrap_err("Failed to deserialize mod config")?;
    cfg.dir = path;
    Ok(cfg)
}

/// Pins the DXC library the config names, if any. The library loads on the
/// first compile; without a config path the discovery order in `dxc` applies.
fn pin_dxc(cfg: &ModConfig) {
    if let Some(library) = &cfg.dxc {
        shader_compile::set_library(library);
    }
}

/// Compiles one HLSL entry point to a container.
#[tracing::instrument(skip_all, fields(source = %source.display(), entry, target))]
async fn compile_hlsl(source: &Path, entry: &str, target: &str) -> Result<Vec<u8>> {
    let text = fs::read_to_string(source)
        .await
        .wrap_err_with(|| format!("Failed to read '{}'", source.display()))?;

    let target_arg = target.to_string();
    let entry_arg = entry.to_string();
    let data =
        tokio::task::spawn_blocking(move || shader_compile::compile(&text, &target_arg, &entry_arg))
            .await
            .wrap_err("The shader compiler task panicked")??;

    tracing::info!(
        "Compiled '{}' ({target}/{entry}, {} bytes)",
        source.display(),
        data.len()
    );

    Ok(data)
}

/// Looks for shader sources next to a material and compiles them.
///
/// A sibling `.shader_node` declaration (the Stingray dialect) is preferred; its
/// code blocks include from the `.shader_source` libraries under the mod root.
/// The older layouts are the fallback: a single `<name>.hlsl` containing
/// `vs_main` and/or `ps_main`, or separate `<name>.vs.hlsl` / `<name>.ps.hlsl`
/// files.
async fn compile_shader_overrides(path: &Path, cfg: &ModConfig) -> Result<Option<ShaderOverrides>> {
    pin_dxc(cfg);

    // A material whose SJSON carries a shader graph: its declaration is the
    // graph's output node - loaded from the mod root, like every definition the
    // graph names - and the graph itself is generated into it.
    if path.extension().is_some_and(|extension| extension == "material") {
        let text = fs::read_to_string(path)
            .await
            .wrap_err_with(|| format!("Failed to read '{}'", path.display()))?;
        if Graph::from_material(&text)?.is_some() {
            return Ok(Some(compile_graph_material(path, &text, cfg).await?));
        }
    }

    let stem = path.with_extension("");
    let declaration = stem.with_extension("shader_node");
    if declaration.exists() {
        return Ok(Some(compile_declaration(&declaration, cfg, None).await?));
    }

    let combined = stem.with_extension("hlsl");
    let vs_path = stem.with_extension("vs.hlsl");
    let ps_path = stem.with_extension("ps.hlsl");

    if !combined.exists() && !vs_path.exists() && !ps_path.exists() {
        return Ok(None);
    }

    let mut overrides = ShaderOverrides::default();

    if combined.exists() {
        let source = fs::read_to_string(&combined)
            .await
            .wrap_err_with(|| format!("Failed to read '{}'", combined.display()))?;

        if source.contains("vs_main") {
            overrides.vertex = Some(compile_hlsl(&combined, "vs_main", "vs_6_0").await?);
        }
        if source.contains("ps_main") {
            overrides.pixel = Some(compile_hlsl(&combined, "ps_main", "ps_6_0").await?);
        }

        if overrides.is_empty() {
            eyre::bail!(
                "'{}' defines neither 'vs_main' nor 'ps_main'",
                combined.display()
            );
        }
    }

    if vs_path.exists() {
        overrides.vertex = Some(compile_hlsl(&vs_path, "vs_main", "vs_6_0").await?);
    }
    if ps_path.exists() {
        overrides.pixel = Some(compile_hlsl(&ps_path, "ps_main", "ps_6_0").await?);
    }

    Ok(Some(overrides))
}

/// Compiles a material whose SJSON carries a shader graph. The graph's output
/// node is the declaration; every definition the graph names is read from the
/// mod root at the path the graph writes, so a mod ships the node definitions
/// its material uses (`core/shader_nodes/...` and the output node).
async fn compile_graph_material(
    material: &Path,
    text: &str,
    cfg: &ModConfig,
) -> Result<ShaderOverrides> {
    let graph = Graph::from_material(text)?
        .ok_or_else(|| eyre::eyre!("'{}' has no shader graph", material.display()))?;

    let mut defs = BTreeMap::new();
    for node in &graph.nodes {
        let path = cfg.dir.join(format!("{}.shader_node", node.kind));
        let definition = fs::read_to_string(&path)
            .await
            .wrap_err_with(|| format!("Failed to read '{}'", path.display()))?;
        let definition = NodeDef::from_text(&definition)
            .wrap_err_with(|| format!("Failed to parse '{}'", path.display()))?;
        defs.insert(node.kind.clone(), definition);
    }

    let output = graph
        .output_node()
        .ok_or_else(|| eyre::eyre!("'{}' has no output node", material.display()))?;
    let declaration = cfg.dir.join(format!("{}.shader_node", output.kind));
    let shader_inputs: BTreeMap<String, String> = defs
        .get(&output.kind)
        .map(|definition| {
            definition
                .inputs
                .iter()
                .map(|(uuid, input)| (uuid.clone(), input.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let resolution = graph.resolve(&defs, &shader_inputs)?;
    let evaluation = resolution.evaluate(&defs)?;
    tracing::info!(
        "'{}' graph: {} nodes, {} channels, {} samplers, {} defines, \
         {} vertex and {} pixel bytes of evaluation",
        material.display(),
        graph.nodes.len(),
        evaluation.channels.len(),
        evaluation.samplers.len(),
        evaluation.defines.len(),
        evaluation.vertex.len(),
        evaluation.pixel.len()
    );

    compile_declaration(&declaration, cfg, Some(&evaluation)).await
}

/// Compiles a `.shader_node` declaration into the stage overrides a material
/// needs.
///
/// The declaration's single job is assembled per stage against the
/// `.shader_source` libraries under the mod root and compiled with DXC. A
/// declaration with more than one job is refused: the override flow names one
/// container per stage, and mapping several jobs to a section's programs needs
/// the conditions decode.
async fn compile_declaration(
    declaration: &Path,
    cfg: &ModConfig,
    evaluation: Option<&Evaluation>,
) -> Result<ShaderOverrides> {
    let text = fs::read_to_string(declaration)
        .await
        .wrap_err_with(|| format!("Failed to read '{}'", declaration.display()))?;
    let node = ShaderNode::from_sjson(&text)
        .wrap_err_with(|| format!("Failed to parse '{}'", declaration.display()))?;

    let jobs = node.compile_jobs()?;
    if jobs.len() != 1 {
        eyre::bail!(
            "'{}' has {} compile jobs; the material flow replaces one program pair per \
             stage, so the declaration must have exactly one pass for now",
            declaration.display(),
            jobs.len()
        );
    }
    let job = &jobs[0];

    let libraries = load_libraries(&cfg.dir)?;

    let mut overrides = ShaderOverrides::default();
    for stage in STAGES {
        let Some(profile) = profile_for(stage) else {
            continue;
        };
        let Some(entry) = entry_for(profile) else {
            continue;
        };
        let source = node.job_source(job, stage, &libraries, evaluation);
        let profile_arg = profile.to_string();
        let entry_arg = entry.to_string();
        let container =
            tokio::task::spawn_blocking(move || shader_compile::compile(&source, &profile_arg, &entry_arg))
                .await
                .wrap_err("The shader compiler task panicked")??;

        // What the container binds, from its own reflection: the per-stage
        // attribution the source text cannot give.
        if let Some(stage) = match stage {
            "vertex" => Some(Stage::Vertex),
            "pixel" => Some(Stage::Pixel),
            _ => None,
        } {
            match StageResources::from_container(&container) {
                Ok(resources) => {
                    overrides.resources.insert(stage, resources);
                }
                Err(err) => tracing::warn!("no reflection for the {stage:?} stage: {err}"),
            }
        }

        tracing::info!(
            "Compiled '{}' ({} block '{}', {profile}/{entry}, {} bytes)",
            declaration.display(),
            stage,
            job.code_block,
            container.len()
        );

        match stage {
            "vertex" => overrides.vertex = Some(container),
            "pixel" => overrides.pixel = Some(container),
            _ => {}
        }
    }

    Ok(overrides)
}

/// Every `.shader_source` under the mod root, parsed as a library, in path
/// order.
fn load_libraries(root: &Path) -> Result<Vec<ShaderSource>> {
    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .wrap_err_with(|| format!("Failed to read '{}'", dir.display()))?;
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "shader_source") {
                paths.push(path);
            }
        }
    }
    paths.sort();

    let mut libraries = Vec::with_capacity(paths.len());
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("Failed to read '{}'", path.display()))?;
        libraries.push(
            ShaderSource::from_sjson(&text)
                .wrap_err_with(|| format!("Failed to parse '{}'", path.display()))?,
        );
    }
    Ok(libraries)
}

/// Reads the `shader_engine_data = "..."` declaration of a material SJSON, if
/// it has one. The field is DTMT's own; the material parser ignores it.
fn shader_engine_data_path(sjson: &str) -> Option<String> {
    for line in sjson.lines() {
        let Some(rest) = line.trim().strip_prefix("shader_engine_data") else {
            continue;
        };
        let rest = rest.trim_start().strip_prefix('=')?.trim();
        let rest = rest.strip_prefix('"')?;
        return rest.split('"').next().map(str::to_string);
    }
    None
}

/// Resolves an engine data path next to the material, then against the mod root.
fn resolve_engine_data_path(material: &Path, root: &Path, name: &str) -> PathBuf {
    let sibling = material
        .parent()
        .unwrap_or(Path::new("."))
        .join(name);
    if sibling.exists() { sibling } else { root.join(name) }
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02X}");
    }
    text
}

/// Resolve a mod file name to the bundle name, applying `name_overrides`.
fn apply_name_override(name_overrides: &HashMap<String, String>, name: String) -> IdString64 {
    if let Some(new_name) = name_overrides.get(&name) {
        let resolved = match u64::from_str_radix(new_name, 16) {
            Ok(hash) => IdString64::from(hash),
            Err(_) => IdString64::from(new_name.clone()),
        };
        tracing::info!("Overriding '{}' -> '{}'", name, resolved.display());
        resolved
    } else {
        IdString64::from(name)
    }
}

/// Iterate over the paths in the given `Package` and
/// compile each file by its file type.
#[tracing::instrument(skip_all)]
async fn compile_package_files(pkg: &Package, cfg: &ModConfig) -> Result<Vec<BundleFile>> {
    let root = Arc::new(&cfg.dir);
    let name_overrides = &cfg.name_overrides;

    let tasks = pkg
        .iter()
        .flat_map(|(file_type, names)| {
            names.iter().map(|name| {
                (
                    *file_type,
                    name,
                    // Cloning the `Arc` here solves the issue that in the next `.map`, I need to
                    // `move` the closure parameters, but can't `move` `root` before it was cloned.
                    root.clone(),
                )
            })
        })
        .map(|(file_type, name, root)| async move {
            let path = PathBuf::from(name);

            // A `.unit` is compiled from the SJSON source and the `.bsi`
            // geometry beside it, mirroring the SDK workflow.
            if file_type == BundleFileType::Unit {
                let unit_sjson = fs::read_to_string(&path)
                    .await
                    .wrap_err_with(|| format!("Failed to read file '{}'", path.display()))?;
                let bsi_path = path.with_extension("bsi");
                let bsi = fs::read(&bsi_path).await.wrap_err_with(|| {
                    format!(
                        "Unit '{}' has no sibling BSI geometry at '{}'",
                        path.display(),
                        bsi_path.display()
                    )
                })?;
                let name = apply_name_override(
                    name_overrides,
                    path.with_extension("").to_slash_lossy().to_string(),
                );
                return sdk::filetype::unit::compile(name, &unit_sjson, &bsi);
            }

            let mut sjson = fs::read_to_string(&path)
                .await
                .wrap_err_with(|| format!("Failed to read file '{}'", path.display()))?;

            // The material's resource name is the section's identity: its
            // material-hash word is murmur32 of this path.
            let identity = path.with_extension("").to_slash_lossy().to_string();

            // A material can declare the engine data to generate from; the
            // section is built in memory and handed to the material compile, so
            // neither the material source nor a side file carries it. Sibling
            // shader sources replace the programs; without them the section is
            // carried as it is (the engine data's own programs, verbatim),
            // which is what a mod that only tweaks the conditions or group data
            // wants. A material with no engine data compiles as it is (its own
            // `shader_data` field or the splice route).
            let mut section: Option<Vec<u8>> = None;
            let mut generated_from_engine_data = false;
            if file_type == BundleFileType::Material
                && let Some(engine_data_name) = shader_engine_data_path(&sjson)
            {
                let engine_data_path =
                    resolve_engine_data_path(&path, root.as_ref(), &engine_data_name);
                let engine_data_text =
                    fs::read_to_string(&engine_data_path).await.wrap_err_with(|| {
                        format!(
                            "Failed to read engine data '{}'",
                            engine_data_path.display()
                        )
                    })?;
                let engine_data =
                    EngineData::from_text(&engine_data_text).wrap_err_with(|| {
                        format!(
                            "Failed to parse engine data '{}'",
                            engine_data_path.display()
                        )
                    })?;

                let overrides = compile_shader_overrides(&path, cfg).await?.unwrap_or_default();

                let carried = overrides.is_empty();
                let resources = overrides.resources.clone();
                let mut containers = HashMap::new();
                if let Some(vertex) = overrides.vertex {
                    containers.insert(Stage::Vertex, vertex);
                }
                if let Some(pixel) = overrides.pixel {
                    containers.insert(Stage::Pixel, pixel);
                }

                let generated = engine_data
                    .generate(&containers, &identity, &resources)
                    .wrap_err_with(|| {
                        format!("Failed to generate a shader section for '{}'", path.display())
                    })?;

                tracing::info!(
                    "{} a {} byte shader section from '{}'",
                    if carried { "Carried" } else { "Generated" },
                    generated.len(),
                    engine_data_path.display(),
                );
                section = Some(generated);
                generated_from_engine_data = true;
            }

            let name = apply_name_override(
                name_overrides,
                path.with_extension("").to_slash_lossy().to_string(),
            );
            let mut file = match &section {
                Some(section) => {
                    material::compile_with_shader(name, &sjson, Some(section)).wrap_err_with(
                        || format!("Failed to compile '{}'", path.display()),
                    )?
                }
                None => BundleFile::from_sjson(name, file_type, sjson, root.as_ref()).await?,
            };

            // The splice route replaces programs in the material's own section,
            // so it only applies when the section was not just generated.
            if !generated_from_engine_data
                && file_type == BundleFileType::Material
                && let Some(overrides) = compile_shader_overrides(&path, cfg).await?
            {
                material::apply_shader_overrides(&mut file, &overrides)?;
            }

            Ok(file)
        });

    let results = futures::stream::iter(tasks)
        .buffer_unordered(10)
        .collect::<Vec<Result<BundleFile>>>()
        .await;

    results.into_iter().collect()
}

/// Read a `.package` file, collect the referenced files
/// and compile all of them into a bundle.
#[tracing::instrument]
async fn build_package(
    cfg: &ModConfig,
    package: impl AsRef<Path> + std::fmt::Debug,
) -> Result<Bundle> {
    let root = &cfg.dir;
    let package = package.as_ref();

    let mut path = root.join(package);
    path.set_extension("package");
    let sjson = fs::read_to_string(&path)
        .await
        .wrap_err_with(|| format!("Failed to read file {}", path.display()))?;

    let pkg_name = package.to_slash_lossy().to_string();
    let pkg = Package::from_sjson(sjson, pkg_name.clone(), root)
        .await
        .wrap_err_with(|| format!("Invalid package file {}", &pkg_name))?;

    let files = compile_package_files(&pkg, cfg).await?;
    let mut bundle = Bundle::new(pkg_name);
    for file in files {
        bundle.add_file(file);
    }

    Ok(bundle)
}

/// Writes any external data files referenced by the bundle's variants, such as
/// streamed texture mipmaps, relative to the given base directory.
///
/// When `backup` is set (deploying into the game), existing files are backed up
/// to a `.bak` sibling before being overwritten, so a deployment can be undone.
#[tracing::instrument(skip_all, fields(base = %base.as_ref().display(), backup))]
async fn write_external_data_files(
    bundle: &Bundle,
    base: impl AsRef<Path> + std::fmt::Debug,
    backup: bool,
) -> Result<()> {
    let base = base.as_ref();

    for file in bundle.files() {
        for variant in file.variants() {
            let (Some(name), Some(data)) = (variant.data_file_name(), variant.external_data())
            else {
                continue;
            };

            let path = base.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .wrap_err_with(|| format!("Failed to create '{}'", parent.display()))?;
            }

            tracing::trace!(path = %path.display(), "Writing external data file");

            if backup {
                write_deployed_file(&path, data).await?;
            } else {
                fs::write(&path, data)
                    .await
                    .wrap_err_with(|| format!("Failed to write '{}'", path.display()))?;
            }
        }
    }

    Ok(())
}

/// Cleans the path of internal parent (`../`) or self (`./`) components,
/// and ensures that it is relative.
fn normalize_file_path<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let path = path.as_ref();

    if path.is_absolute() || path.has_root() {
        let err = eyre::eyre!("Path is absolute: {}", path.display());
        return Err(err).with_suggestion(|| "Specify a relative file path.".to_string());
    }

    let path = path_clean::clean(path);

    if path.starts_with("..") {
        eyre::bail!("path starts with a parent component: {}", path.display());
    }

    Ok(path)
}

#[tracing::instrument]
pub(crate) async fn read_project_config(dir: Option<PathBuf>) -> Result<ModConfig> {
    let mut cfg = find_project_config(dir).await?;

    if let Some(path) = cfg.image {
        let path = normalize_file_path(path)
            .wrap_err("Invalid config field 'image'")
            .with_suggestion(|| {
                "Specify a file path relative to and child path of the \
                    directory where 'dtmt.cfg' is."
                    .to_string()
            })?;
        cfg.image = Some(path);
    }

    cfg.resources.init = normalize_file_path(cfg.resources.init)
        .wrap_err("Invalid config field 'resources.init'")
        .with_suggestion(|| {
            "Specify a file path relative to and child path of the \
                    directory where 'dtmt.cfg' is."
                .to_string()
        })
        .with_suggestion(|| {
            "Use 'dtmt new' in a separate directory to generate \
                    a valid mod template."
                .to_string()
        })?;

    if let Some(path) = cfg.resources.data {
        let path = normalize_file_path(path)
            .wrap_err("Invalid config field 'resources.data'")
            .with_suggestion(|| {
                "Specify a file path relative to and child path of the \
                            directory where 'dtmt.cfg' is."
                    .to_string()
            })
            .with_suggestion(|| {
                "Use 'dtmt new' in a separate directory to generate \
                            a valid mod template."
                    .to_string()
            })?;
        cfg.resources.data = Some(path);
    }

    if let Some(path) = cfg.resources.localization {
        let path = normalize_file_path(path)
            .wrap_err("Invalid config field 'resources.localization'")
            .with_suggestion(|| {
                "Specify a file path relative to and child path of the \
                        directory where 'dtmt.cfg' is."
                    .to_string()
            })
            .with_suggestion(|| {
                "Use 'dtmt new' in a separate directory to generate \
                        a valid mod template."
                    .to_string()
            })?;
        cfg.resources.localization = Some(path);
    }

    Ok(cfg)
}

#[tracing::instrument]
pub(crate) async fn build<P>(
    cfg: &ModConfig,
    out_path: impl AsRef<Path> + std::fmt::Debug,
    game_dir: Arc<Option<P>>,
    force: bool,
) -> Result<()>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let out_path = out_path.as_ref();

    if let Some(dir) = game_dir.as_ref() {
        let dir = dir.as_ref();
        let marker = dir.parent().map(|parent| parent.join(DEPLOYMENT_MARKER));

        if !force
            && let Some(marker) = marker
            && fs::try_exists(&marker).await.unwrap_or(false)
        {
            return Err(eyre::eyre!(
                "A mod deployment is already active (found '{}')",
                marker.display()
            ))
            .with_suggestion(|| {
                "Run `dtmm --reset` first so originals are restored before deploying \
                 again, or pass --force to deploy anyway."
                    .to_string()
            });
        }
    }

    fs::create_dir_all(out_path)
        .await
        .wrap_err_with(|| format!("Failed to create output directory '{}'", out_path.display()))?;

    let file_map = Arc::new(Mutex::new(FileIndexMap::new()));

    let tasks = cfg
        .packages
        .iter()
        // The closure below would capture the `Arc`s before they could be cloned,
        // so instead we need to clone them in a non-move block and inject them
        // via parameters.
        .map(|path| (path, cfg.clone(), file_map.clone(), game_dir.clone()))
        .map(|(path, cfg, file_map, game_dir)| async move {
            if path.extension().is_some() {
                eyre::bail!(
                    "Package name must be specified without file extension: {}",
                    path.display()
                );
            }

            let bundle = build_package(&cfg, path).await.wrap_err_with(|| {
                format!(
                    "Failed to build package '{}' at '{}'",
                    path.display(),
                    cfg.dir.display()
                )
            })?;

            let bundle_name = match bundle.name() {
                IdString64::Hash(_) => {
                    eyre::bail!("bundle name must be known as string. got hash")
                }
                IdString64::String(s) => s.clone(),
            };

            {
                let mut file_map = file_map.lock().await;
                let map_entry = file_map.entry(bundle_name).or_default();

                for file in bundle.files() {
                    map_entry.insert(file.name(false, None));
                }
            }

            let name = bundle.name().to_murmur64().to_string().to_ascii_lowercase();
            let path = out_path.join(&name);
            let data = bundle.to_binary()?;

            tracing::trace!(
                "Writing bundle {} to '{}'",
                bundle.name().display(),
                path.display()
            );
            fs::write(&path, &data)
                .await
                .wrap_err_with(|| format!("Failed to write bundle to '{}'", path.display()))?;

            write_external_data_files(&bundle, out_path, false).await?;

            if let Some(game_dir) = game_dir.as_ref() {
                let path = game_dir.as_ref().join(&name);

                tracing::trace!(
                    "Deploying bundle {} to '{}'",
                    bundle.name().display(),
                    path.display()
                );
                write_deployed_file(&path, &data).await?;

                write_external_data_files(&bundle, game_dir.as_ref(), true).await?;
            }

            Ok(())
        });

    try_join_all(tasks)
        .await
        .wrap_err("Failed to build mod bundles")?;

    {
        let path = out_path.join("files.sjson");
        tracing::trace!(path = %path.display(), "Writing file index");
        let file_map = file_map.lock().await;
        let data = serde_sjson::to_string(file_map.deref())?;
        fs::write(&path, data)
            .await
            .wrap_err_with(|| format!("Failed to write file index to '{}'", path.display()))?;
    }

    if let Some(img_path) = &cfg.image {
        let path = cfg.dir.join(img_path);
        let dest = out_path.join(img_path);

        tracing::trace!(src = %path.display(), dest = %dest.display(), "Copying image file");

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(&parent)
                .await
                .wrap_err_with(|| format!("Failed to create directory '{}'", parent.display()))?;
        }

        fs::copy(&path, &dest).await.wrap_err_with(|| {
            format!(
                "Failed to copy image from '{}' to '{}'",
                path.display(),
                dest.display()
            )
        })?;
    }

    tracing::info!("Compiled bundles written to '{}'", out_path.display());

    if let Some(game_dir) = game_dir.as_ref() {
        tracing::info!("Deployed bundles to '{}'", game_dir.as_ref().display());
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let cfg = read_project_config(matches.get_one::<PathBuf>("directory").cloned()).await?;

    let game_dir = matches
        .get_one::<PathBuf>("deploy")
        .map(|p| p.join("bundle"));

    let out_path = matches
        .get_one::<PathBuf>("out")
        .expect("parameter should have default value");

    let force = matches.get_flag("force");

    tracing::debug!(?cfg, ?game_dir, ?out_path);

    let game_dir = Arc::new(game_dir);

    build(&cfg, out_path, game_dir, force).await?;

    Ok(())
}
