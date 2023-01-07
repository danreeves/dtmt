use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{self, Context};
use color_eyre::Result;
use sdk::murmur::Murmur64;
use sdk::Bundle;
use zip::ZipWriter;

pub struct Archive {
    name: String,
    bundles: Vec<Bundle>,
    mod_file: Option<Vec<u8>>,
}

impl Archive {
    pub fn new(name: String) -> Self {
        Self {
            name,
            bundles: Vec::new(),
            mod_file: None,
        }
    }

    pub fn add_bundle(&mut self, bundle: Bundle) {
        self.bundles.push(bundle)
    }

    pub fn add_mod_file(&mut self, content: Vec<u8>) {
        self.mod_file = Some(content);
    }

    pub fn write<P>(&self, ctx: &sdk::Context, path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let mod_file = self
            .mod_file
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Mod file is missing from mod archive"))?;

        let f = File::create(path.as_ref()).wrap_err_with(|| {
            format!(
                "failed to open file for reading: {}",
                path.as_ref().display()
            )
        })?;
        let mut zip = ZipWriter::new(f);

        zip.add_directory(&self.name, Default::default())?;

        let base_path = PathBuf::from(&self.name);

        {
            let mut name = base_path.join(&self.name);
            name.set_extension("mod");
            zip.start_file(name.to_string_lossy(), Default::default())?;
            zip.write_all(mod_file)?;
        }

        let mut file_map = HashMap::new();

        for bundle in self.bundles.iter() {
            let bundle_name = bundle.name().clone();

            let map_entry: &mut HashSet<_> = file_map.entry(bundle_name).or_default();

            for file in bundle.files() {
                map_entry.insert(file.name(false, None));
            }

            let name = Murmur64::hash(bundle.name().as_bytes());
            let path = base_path.join(name.to_string().to_ascii_lowercase());

            zip.start_file(path.to_string_lossy(), Default::default())?;

            let data = bundle.to_binary(ctx)?;
            zip.write_all(&data)?;
        }

        {
            let data = serde_sjson::to_string(&file_map)?;
            zip.start_file(
                base_path.join("files.sjson").to_string_lossy(),
                Default::default(),
            )?;
            zip.write_all(data.as_bytes())?;
        }

        zip.finish()?;

        Ok(())
    }
}
