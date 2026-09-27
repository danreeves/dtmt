# Legacy Mod Compatibility

Status: draft

Premise: DML is no longer required as a component. DTMM keeps owning the boot path and the mod
loader; real DMF and unmodified legacy (DML/DMF-era) mods must keep loading. The goal is a
transition period during which users port mods to DTMT at their own pace.

Paths are relative to the repository root at the time of writing. Where a file has moved, use the
named component as the reference.

## Goals

- DTMM's boot stays: `application_settings/settings_common.ini` -> `scripts/mod_main`, the
  `BootStateLoadMods` injection, and the `packages/mods` loader bundle.
- Real DMF loads as an ordinary mod and provides `new_mod`, `get_mod`, hooks, options, mutators.
- Legacy `.mod` mods load from `mods/<name>/<name>.mod` with DML's semantics.
- Load order is `[dmf] + [DTMM mods] + [legacy mods]`; the legacy block is driven by the live
  `mods/mod_load_order.txt`.
- Existing DML installations can be taken over without losing mods or their order; `mods/` and
  `mod_load_order.txt` keep working for users and tools that still use the DML workflow.
- DTMT mods and legacy mods run side by side during the transition.

## Non-goals

- Running DML's `ModManager`, `binaries/mod_loader`, or `mods/base`.
- Managing, reordering, or dependency-validating legacy mods inside DTMM.
- Automatic conversion of legacy mods to DTMT projects.

## Compatibility surface

DMF and legacy mods need the following from the loader environment. This list defines what the
compatibility layer must provide.

| Surface | Used by | Notes |
|---|---|---|
| `Mods.file.{exec, exec_with_return, exec_unsafe, exec_unsafe_with_return, dofile, read_content, read_content_to_table, exists}` | DMF (`dmf.mod`, `dmf_loader`), legacy mods | io relative to `./../mods` |
| `Mods.lua.{debug, io, os, loadstring, ffi}` | DMF modules, legacy mods | always present |
| `Mods.require_store`, `Mods.original_require` | DMF's require module | DTMM already installs the require wrapper |
| `CLASS`, wrapped `class`, `Mods.original_class` | DMF hooks/options, legacy mods | DTMM already wraps `class` |
| `Mods.hook` (DML hook v2) | legacy mods | not used by DMF |
| `Mods.message` | legacy mods | notify/echo |
| `Managers.mod._mods`, `._mod_load_index`, `._state`, `._settings`, `._reload_requested` | DMF (`dmf_mod_data`, `dmf_options`, `dmf_loader`) | loader facade |
| `CLASS.ModManager` with `destroy` and `_check_reload` | DMF (`dmf_loader`, `dmf_options`) | loader class naming |
| Mod entry fields `id`, `name`, `handle`, `data` | DMF (`dmf_mod_data`) | `name` is used for `info.json`; `data.packages` is read during `new_mod` |
| `__print` | DMF modules | provided by the game's `scripts/foundation/utilities/log` |

## Design

### Loader

- Name the loader class `ModManager` so `CLASS.ModManager` resolves for DMF. Keep the require
  wrapper and class wrapping already present in `scripts/mod_main`.
- Mod entries become DML-shaped: `name` = load-order/folder name, `handle`, `enabled`, `object`,
  `data`. The DTMT id remains available for `new_mod`/`get_mod`.
- Add a legacy branch to the loader state machine:
  1. Load `mods/<name>/<name>.mod` via `Mods.file.exec_with_return(name, name, "mod")`.
  2. Store the returned table as `mod.data` before running it (DMF reads `data.packages` in
     `DMFMod:init`).
  3. Call `data.run()` in the initialization step and assign `mod.object`.
- Lifecycle parity with DML's ModManager: `init`, `update`, `on_reload` before rescan, `on_unload`
  on unload, `on_destroy`, `on_game_state_changed`. Unload in reverse order.
- Settings: read `mod_manager_settings` (the key DMF reads and writes), include `developer_mode`
  and `log_level`, and honor `_reload_requested`.
- Package ownership:
  - Bundled DTMT mods: the loader loads packages as today (wait for `has_loaded`, flush) before
    `run`; hide `data.packages` from DMF afterwards so it does not start a second async load.
  - Loose mods (managed or unmanaged): packages declared in their `.mod` file are DMF's
    responsibility.

### Load order

The deployed list is composed as three blocks:

```
[1] dmf         pinned first, whether DTMM-managed or a loose folder at mods/dmf
[2] dtmm mods   generated mod_data, DTMM's order and dependency checks
[3] legacy      read at runtime from mods/mod_load_order.txt, in file order
```

- `mod_load_order.txt` is never written by DTMM. It stays the live contract for the legacy block.
- The loader reads it after the generated list via
  `Mods.file.read_content_to_table("mod_load_order", "txt")`. Parsing matches DML: trim lines,
  ignore empty lines and lines starting with `--`.
- Skip rules: `dmf` entries; names already loaded from block 2; entries whose folder or `.mod` file
  is missing (log and continue, matching DML's "missing mod" behavior).
- Duplicate lines: first occurrence wins, warn.
- Reload (`ctrl+shift+R`) re-reads the file, matching DML's rescan.
- Adding or removing a legacy mod needs no DTMM deploy; editing the file and dropping a folder is
  enough before the next launch.
- Porting consequence: a ported mod moves from block 3 to block 2, i.e. earlier in the chain. Later
  loads win direct function assignment, so this can change precedence for override-style mods.
  Document it; do not enforce it.

### Compatibility API in the boot script

`crates/dtmm/assets/mod_main.lua.j2`:

- Always provide the full `Mods.lua` table: `debug`, `io`, `os`, `loadstring`, `ffi`. Remove the
  `is_io_enabled` gate and its plumbing:
  - `assets/mod_main.lua.j2` (template token),
  - `crates/dtmm/src/controller/deploy.rs` (render context),
  - `crates/dtmm/src/state/data.rs`, `state/delegate.rs`, `util/config.rs` (`unsafe_io`),
  - `crates/dtmm/src/ui/window/main.rs` (checkbox), `ui/widget/controller.rs` (dirty check).
- Add `Mods.file`, `Mods.hook`, `Mods.message`, and `Mods.original_class`.
- Remove the `new_mod`/`get_mod` shim from `crates/dtmm/assets/init.lua`. Keep the loader bootstrap
  and the state hooks.

### DMF dependency policy

- Dependencies stay author-declared (`depends` in `dtmt.cfg`). DTMM does not infer DMF usage from
  resource files.
- DMF is pinned first whenever it is present, declared or not.
- A mod whose generated entry calls `new_mod` fails inside its own `run` if DMF is absent; the
  loader isolates that mod (xpcall, callbacks disabled) and continues.
- The generated entry includes a guard that raises a clear error instead of
  "attempt to call global 'new_mod'":
  `if not new_mod then error("[DTMM] Mod '<id>' requires Darktide Mod Framework (DMF).", 2) end`
- Mods without `data`/`localization` keep the no-DMF path (`dofile(init)` / `require(init)`).

### Generated mod_data

`crates/dtmm/assets/mod_data.lua.j2` gains:

- `load_order_name` (folder/load-order identity, also used for DMF's `info.json` lookup),
- `legacy`/`mod_file` markers where relevant,
- function resources for bundled mods when calling `new_mod`:
  `mod_script = function() require("<path>") end`, `mod_data = function() return require("<path>") end`,
  and so on. Real DMF resolves strings through io, which cannot find files inside bundles;
  functions work with DMF and with the loader's own entry generation.

## Work items

### Lua assets

- [ ] `mod_main`: full `Mods.lua`, `Mods.file`, `Mods.hook`, `Mods.message`, `Mods.original_class`.
- [ ] `init.lua`: remove the shim.
- [ ] `mod_loader`: `ModManager` naming, DML-shaped entries, legacy branch, lifecycle, settings,
      package ownership, legacy block reading, DMF pinning.
- [ ] `mod_data` template: new fields, function resources, DMF guard.

### Rust

- [ ] Remove `is_io_enabled`/`unsafe_io` plumbing.
- [ ] Keep boot deployment unchanged (settings, boot bundle, `packages/mods`).
- [ ] Loose mod copy destination uses `load_order_name`.
- [ ] Import: `.mod` introspection fallback (run-style files such as DMF's), keep the original
      tree, record `load_order_name`, flag `dmf` as a framework mod.
- [ ] Migration: replace the hard "Found dtkit-patch-based mod installation" error and the
      destructive reset behavior with an explicit migration action:
  - leave `mods/` and `mod_load_order.txt` untouched,
  - restore `bundle_database.data` from `bundle_database.data.bak`,
  - remove `9ba626afa44a3aa3.patch_999`, `binaries/mod_loader`, `mods/base`, `tools/`,
    `toggle_darktide_mods.bat`, `README.md`,
  - keep a rollback copy until the user confirms,
  - deploy DTMM normally afterwards.
- [ ] UI: read-only legacy mod list from the game directory and the order file, warnings (missing
      folder, duplicates, `dmf` listed), import/port action.

### Docs

- [ ] `docs/Bundle-Patcher-Architecture.md`: the loader now hosts legacy mods.
- [ ] `docs/Installing-mods-with-DTMM.md`: migration instructions instead of reset instructions.
- [ ] `docs/dtmt.cfg-Reference.md`: DMF dependency guidance.
- [ ] `CHANGELOG.adoc`: no more DMF-free claims, Lua libs always available.

## Phases

- **M0 - spike.** Lua-only: a minimal `Mods.file` implementation, the legacy branch, and a
  `ModManager` alias in a scratch build. Run real DMF from `mods/dmf`, one legacy `.mod` mod, and
  one bundled mod in game. Confirms the facade fields and package handling before the assets
  rewrite.
- **M1 - assets.** Land the loader/compat assets, ungate the Lua libraries, remove the shim,
  update the generated `mod_data`.
- **M2 - legacy block and import.** Runtime read of `mod_load_order.txt`, DMF resolution/pinning,
  import fallback for run-style `.mod` files, `load_order_name`, read-only UI list.
- **M3 - migration.** dtkit/DML takeover flow, rollback path, docs, changelog.
- **M4 - porting UX.** Format badges, port action (`dtmt migrate` -> build/package -> import),
  cleanup of the transitional UI.

## Test matrix

| Case | Expectation |
|---|---|
| DMF only | loads first; options view works; `new_mod`/`get_mod` available |
| Legacy mod only | loads from `.mod` in file order; `Mods.file` and `Mods.hook` work |
| Legacy + DMF | legacy mod registers via DMF; metadata read from `mods/<name>/info.json` |
| Bundled + DMF | packages loaded once; function resources resolve; no double load |
| Bundled + legacy + DMF | block order is `dmf`, DTMM mods, legacy mods |
| File edited between runs | new legacy mod loads without a redeploy |
| Duplicate or missing entry | warn and continue |
| DMF disabled | DMF-using mods fail isolated; other mods still load |
| Reload (`ctrl+shift+R`) | legacy file is re-read; DMF teardown and re-init are clean |
| Migrated dtkit install | mods and order intact; no DML files needed |

## Risks

- DML's `Mods.file`/`Mods.hook` have no license file in the DML release repository; porting from
  behavior or asking upstream is safer than vendoring.
- DMF couples to loader internals (`_mods`, `_mod_load_index`, `data.packages`,
  `CLASS.ModManager`); upstream changes can break the facade.
- `on_unload`/`on_reload` parity matters for DMF settings and packages; missing callbacks leave
  stale state or leaked packages.
- Removing the shim changes behavior for mods that relied on `get_mod` without DMF.
- Always-on io/ffi is a security posture change; mods get what DML gave them.
- Double package loading (loader vs DMF) can unload packages early if refcounts are mismatched.
- Legacy mods that introspect `Managers.mod` beyond the documented fields may still break.

## Open decisions

- Port `Mods.file`/`Mods.hook` from behavior, or vendor from DML (license question).
- DMF provisioning: require the user to install it, or offer a helper action.
- DMF default: managed DTMM framework mod, or a loose `mods/dmf` folder.
- Whether porting ever edits `mod_load_order.txt` (default: no; the stale line is inert because
  managed names are skipped in the legacy block).
- Whether the legacy block can be disabled at deploy time, for mod lists that should not pick up
  stray folders.
