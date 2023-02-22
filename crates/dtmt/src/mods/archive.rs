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
    config_file: Option<Vec<u8>>,
}

impl Archive {
    pub fn new(name: String) -> Self {
        Self {
            name,
            bundles: Vec::new(),
            config_file: None,
        }
    }

    pub fn add_bundle(&mut self, bundle: Bundle) {
        self.bundles.push(bundle)
    }

    pub fn add_config(&mut self, content: Vec<u8>) {
        self.config_file = Some(content);
    }

    pub fn write<P>(&self, path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let config_file = self
            .config_file
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Config file is missing in mod archive"))?;

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
            let name = base_path.join("dtmt.cfg");
            zip.start_file(name.to_string_lossy(), Default::default())?;
            zip.write_all(config_file)?;
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

            let data = bundle.to_binary()?;
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
