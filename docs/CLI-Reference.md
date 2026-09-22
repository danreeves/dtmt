# CLI Reference

**Note**: This page is only occasionally updated, and may be out of date. Run `dtmt --help` for the help reference that corresponds to the version you are using.

The general usage is shown with `dtmt --help`:

```none
Usage: dtmt [OPTIONS] <COMMAND>

Commands:
  build       Build a project
  bundle      Manipulate the game's bundle files
  dictionary  Manipulate a hash dictionary file.
  murmur      Perform various operations on Murmur hashes.
  new         Create a new project
  package     Package compiled bundles for distribution
  watch       Watch for file system changes and re-build the mod archive.
  help        Print this message or the help of the given subcommand(s)

Options:
      --dict <dictionary>  Path to a dictionary file CSV format used to look up pre-computed murmur hashes.
                           Will default to `dictionary.csv` in the current directory. [default: dictionary.csv]
  -h, --help               Print help
  -V, --version            Print version
```

**Tip:**
Whenever a subcommand is expected, `help` can be given to produce the same output as including the `--help` flag.

## Global Options

| Flag              | Value     | Description                                                                                      |
|-------------------|-----------|--------------------------------------------------------------------------------------------------|
| `-h`, `--help`    |           | Prints help information about the current command. May also be specified in the command's options. |
| `-V`, `--version` |           | Prints version information.                                                                      |
| `--dict`          | file path | Sets the path to the Murmur hash dictionary. Defaults to `dictionary.csv`.  May also be specified in the command's options.

## Subcommands

### Build

Builds the given mod project.

```
$ dtmt build --help
Usage: dtmt build [OPTIONS] [directory]

Arguments:
  [directory]  The path to the project to build. If omitted, dtmt will search from the current working directory upward.

Options:
      --dict <dictionary>  Path to a dictionary file CSV format used to look up pre-computed murmur hashes.
                           Will default to `dictionary.csv` in the current directory. [default: dictionary.csv]
      --oodle <oodle>      The oodle library to load. This may either be:
                           - A library name that will be searched for in the system's default paths.
                           - A file path relative to the current working directory.
                           - An absolute file path.
  -o, --out <out>          The directory to write output files to. [default: out]
  -d, --deploy <deploy>    If the path to the game (without the trailing '/bundle') is specified, deploy the newly built bundles. This will not adjust the bundle database or package files, so if files are added or removed, you will have to import into DTMM and re-deploy there.
  -h, --help               Print help
```

See [Building your first mod](Building-your-first-mod) for a guide on how to use this.

### Bundle

A collection of commands to operate on data bundles.

```
$ dtmt bundle help
Usage: dtmt bundle [OPTIONS] <COMMAND>

Commands:
  decompress  Create a decompressed version of the given bundle(s).
                  This is mostly useful for staring at the decompressed data in a hex editor,
                  as neither the game nor this tool can read the decompressed bundles.
  extract     Extract files from the bundle(s).
  inject      Inject a file into a bundle.
  list        List the contents of one or multiple bundles.
  help        Print this message or the help of the given subcommand(s)

Options:
      --dict <dictionary>  Path to a dictionary file CSV format used to look up pre-computed murmur hashes.
                           Will default to `dictionary.csv` in the current directory. [default: dictionary.csv]
  -h, --help               Print help information
```

#### Specifying bundle files

Unless specified otherwise, all commands in this group can operate on multiple bundles at once. When the help output shows `bundle...` as a parameter, the command expects any number of file or directory paths.
File paths will be loaded a bundles regardless of their file name. Directories will be searched for files fitting Bitsquid's hashed bundle names (16 hex characters).

Once all parameters have been resolved to bundle files, the operation is executed on all of them.

Therefore, the content of all of the game's bundles can be listed at once, using:

```shell
dtmt bundle list "<path to game>\bundle"
```

#### Specifying command line tools

Certain operations, such as decompiling Lua, require external command line tools. By default, dtmt will attempt to use a binary named like the corresponding flag (e.g. for `--ljd`, the binary name is `ljd`) and expects that to be made available via PATH.

Alternatively, the flag can be used to point to a different executable. It accepts any of the following forms:

* A single file name: A binary with that name must be available via PATH
* A relative or absolute path to a file: Use the binary as-is
  * Special case: If the file is named `main.py`, a binary called `python` is expected to be available via PATH
* A command string: A single string with executable and arguments separated by whitespace. See [shlex](https://docs.rs/shlex/latest/shlex/index.html) for information on parsing and compatibility.

#### Bundle Decompress

```
dtmt bundle decompress [OPTIONS] <bundle>... <destination>
```

Decompresses the raw byte stream of the bundle and writes it to a file with the rest of the bundle's metadata.
This is primarily intended for inspecting the bundle data manually, as neither the game nor this tool can otherwise use the decompressed file.

#### Bundle Extract

```
dtmt bundle extract [OPTIONS] <bundle>... <destination>
```

Extracts files from the given bundle(s).

| Flag                | Value     | Description                                                                                                               |
|---------------------|-----------|---------------------------------------------------------------------------------------------------------------------------|
| `-i`, `--include`   | file glob | Only extract files matching the given glob pattern. May be specified multiple times.                                      |
| `-e`, `--exclude`   | file glob | Do not extract files matching the given glob pattern. Takes precedence over `--include`. May be specified multiple times. |
| `-f`, `--flatten`   |           | Flatten the directory structure of the extracted files into the file name.                                                |
| `-n`, `--dry-run`   |           | Simulate write operations and log what would have been done.                                                              |
| `-d`, `--decompile` |           | Attempt to decompile files before writing.                                                                                |
| `--ljd`             | file path | Path to custom ljd executable                                                                                             |

#### Bundle Inject

```
dtmt bundle inject [OPTIONS] <bundle> <file>
```

Injects a file into a single bundle. Only one bundle can be specified.

| Flag              | Value     | Description                                                                            |
|-------------------|-----------|----------------------------------------------------------------------------------------|
| `-r`, `--replace` | name      | Replace the named entry in the bundle with the contents of the given file.             |
| `-o`, `--output`  | file path | Path to write the changed bundle to. If omitted, the input bundle will be overwritten. |

#### Bundle List

```
dtmt bundle list [OPTIONS] <bundle>...
```

Lists the contents of the given bundles.

| Flag     | Value | Description                            |
|----------|-------|----------------------------------------|
| `--json` |       | Print output in machine-readable JSON. |


### Dictionary

A collection of commands to operate on the Murmur hash dictionary.

```
Usage: dtmt dictionary [OPTIONS] [COMMAND]

Commands:
  lookup  Lookup a hash in the dictionary.
  add     Add strings to the dictionary. Strings are read line by line from the given file.
  save    Save back the currently loaded dictionary, with hashes pre-computed. Pre-computing hashes speeds up loading large dictionaries, as they would otherwise need to be computed on the fly.
  help    Print this message or the help of the given subcommand(s)

Options:
      --dict <dictionary>  Path to a dictionary file CSV format used to look up pre-computed murmur hashes.
                           Will default to `dictionary.csv` in the current directory. [default: dictionary.csv]
  -h, --help               Print help information
```

#### Dictionary Lookup

```
dtmt dictionary lookup [OPTIONS] <hash>
```

Looks up the given hash in the dictionary.

| Flag            | Value | Description                             |
|-----------------|-------|-----------------------------------------|
| `-g`, `--group` | group | Restrict the search to the given group. |

#### Dictionary Add

```
dtmt dictionary add [OPTIONS] <file>
```

Reads `<file>` line-by-line and adds each string into the dictionary.

| Flag            | Value | Description                                              |
|-----------------|-------|----------------------------------------------------------|
| `-g`, `--group` | group | The group to assign the strings to. Defaults to `other`. |

#### Dictionary Save

```
dtmt dictionary save
```

Saves back the currently loaded dictionary, with hashes pre-computed. This is useful after having edited the dictionary file manualy.

### Migrate

Migrates a mod based on the loose file structure to DTMT's structure.

```
Usage: dtmt migrate [OPTIONS] <mod-file> <directory>

Arguments:
  <mod-file>   The path to the mod's '<id>.mod' file.
  <directory>  The directory to create the mod in. Within this directory, DTMT will create a new folder named after the mod ID and migrate files into that folder.

Options:
      --dict <dictionary>  Path to a dictionary file CSV format used to look up pre-computed murmur hashes.
                           Will default to `dictionary.csv` in the current directory. [default: dictionary.csv]
  -h, --help               Print help
```

### Murmur

A collection of commands to operate on Murmur hashes.

```
Usage: dtmt murmur [OPTIONS] [COMMAND]

Commands:
  hash
  help    Print this message or the help of the given subcommand(s)

Options:
      --dict <dictionary>  Path to a dictionary file CSV format used to look up pre-computed murmur hashes.
                           Will default to `dictionary.csv` in the current directory. [default: dictionary.csv]
  -h, --help               Print help information
```

#### Murmur Hash

```
dtmt murmur hash [OPTIONS] <string>
```

Creates a Murmur hash from the given string.

| Flag     | Value | Description                                                                                                   |
|----------|-------|---------------------------------------------------------------------------------------------------------------|
| `--half` |       | Create a Murmur64, using Fatshark's fake 32-bit algorithm that computes the 64-bit hash and takes half of it. |

### New

Creates a new mod project from the internal template.

```
$ dtmt new --help
Usage: dtmt new [OPTIONS] [root]

Arguments:
  [root]  The directory where to initialize the new project. This directory must be empty or must not exist. If omitted or `.` is given, the current directory will be used.

Options:
      --dict <dictionary>  Path to a dictionary file CSV format used to look up pre-computed murmur hashes.
                           Will default to `dictionary.csv` in the current directory. [default: dictionary.csv]
      --name <name>        The display name of the new mod.
  -h, --help               Print help
```

See [Building your first mod](Building-your-first-mod) for a guide on how to use this.

### Watch

Watches the given mod project for file changes and re-builds it.

```
Usage: dtmt watch [OPTIONS] [debounce] [directory]

Arguments:
  [debounce]   The delay to debounce events by. This avoids continously rebuilding on rapid file changes, such as version control checkouts. [default: 150]
  [directory]  The path to the project to build. If omitted, the current working directory is used.

Options:
      --dict <dictionary>  Path to a dictionary file CSV format used to look up pre-computed murmur hashes.
                           Will default to `dictionary.csv` in the current directory. [default: dictionary.csv]
  -o, --out <out>          The directory to write output files to. [default: out]
  -d, --deploy <deploy>    If the path to the game (without the trailing '/bundle') is specified, deploy the newly built bundles. This will not adjust the bundle database or package files, so if files are added or removed, you will have to import into DTMM and re-deploy there.
  -a, --archive <archive>  The path to write the packaged file to. Will default to a file in the current working directory
  -i, --ignore <ignore>    A directory or file path to ignore. May be specified multiple times. The values of 'out' and 'archive' are ignored automatically.
  -h, --help               Print help
```

See [Building your first mod](Building-your-first-mod) for a guide on how to use this.