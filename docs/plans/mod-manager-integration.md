# Mod Manager Integration: Library and Command Line

Status: draft

Premise: DTMM's functionality should be consumable by other mod managers, both as a Rust library
and as a stable headless interface, without duplicating logic in the GUI or in scripting
front-ends.

Paths are relative to the repository root at the time of writing. Where a file has moved, use the
named component as the reference.

## Goals

- A `dtmm-core` library covering game detection, mod storage/import, deployment planning, apply,
  and reset, with no GUI, config-file, or global-state coupling.
- The existing `dtmm` binary keeps working as the GUI and gains a stable, documented command-line
  surface.
- The GUI, the command-line operations, and `dtmt`'s deploy helpers all run the same core code.
- Integrators can query state, import mods, produce a plan without side effects, apply a plan, and
  reset, all with machine-readable output.

## Non-goals

- A new CLI crate.
- Moving Nexus/network access, the IPC server, or config-file handling into the core.
- Rewriting the GUI.

## Design

### Crate layout

```
lib/dtmm-core          new: library
crates/dtmm            application: GUI + command line
crates/dtmt            authoring CLI; uses the core for deployment
```

- `dtmm-core` must not depend on druid, confy, or the application's tracing/log setup. Preferences
  and log sinks are supplied by the caller.
- The command-line interface stays in `crates/dtmm`. A second binary target or druid
  feature-gating can be added later if a lean artifact is needed; it reuses the same core.

### Public API

- Identity and context:
  - `Context { game_dir, store_dir, sdk }`.
  - `ModInfo` and resolved mod sources (config plus files).
- Read-only operations:
  - `detect(&Context) -> Environment` (game present, deployment state, bundle database state, mod
    store contents).
  - `list(&Context) -> Vec<ModInfo>`.
  - `check(&Context) -> Vec<Diagnostic>` (order, dependency, and missing-file checks).
- Mutation planning:
  - `plan(&Context, &PlanRequest) -> DeploymentPlan`: a pure description of the intended changes -
    bundle files to write, database entries, generated assets, mods to copy or remove, files to
    restore.
  - `apply(&Context, &DeploymentPlan, &mut Reporter) -> DeploymentReport`.
  - `reset(&Context, &mut Reporter) -> DeploymentReport`.
  - `import(&Context, archive) -> ModInfo`.
- Events:
  - `DeployEvent` enum (started, built bundle, wrote file, patched database, warning, finished).
  - `Reporter` trait, or a channel sink, so the GUI, the command line, and integrators can render
    progress.
- Errors:
  - `thiserror`-based error enums per operation. `eyre` stays in the binaries.
- State and safety:
  - Deployment data gains a schema version and writer/version fields.
  - A game-directory lock prevents concurrent deployments.
  - No implicit config: callers pass paths and options explicitly.
- Packaging:
  - Cargo features: `oodle`, `nexus`, `blocking`; no GUI dependencies.

### Command line

- Keep `--deploy` and `--reset` with their current behavior and the `deploy-result.txt` output
  until it is intentionally replaced.
- Add subcommands on the same binary: `detect`, `status`, `list`, `import`, `plan`, `deploy`,
  `reset`.
- Common flags: `--game-dir`, `--data-dir`, `--config`, `--log-level`, `--json`, `--dry-run`.
- JSON is the integration contract. Document schemas for `Environment`, `DeploymentPlan`,
  `DeploymentReport`, and `Diagnostic`.
- Stable, documented exit codes per error class.
- Structured results on stdout; human-readable text by default.
- Windows: the binary currently uses the GUI subsystem and has no console. For command-line use,
  either attach to the parent console in command mode or add a console binary target.
- Linux: the GUI binary links druid/GTK. If a lean headless artifact is required, gate the GUI
  behind a feature and ship both variants from the same package.

### Code movement

| Current | Destination |
|---|---|
| `crates/dtmm/src/controller/deploy.rs` | `dtmm-core` plan/apply |
| `crates/dtmm/src/controller/game.rs` | `dtmm-core` detect/reset |
| `crates/dtmm/src/controller/import.rs` | `dtmm-core` import/store |
| `crates/dtmm/src/state/data.rs` core types | `dtmm-core` |
| `crates/dtmm/assets/*.lua*` | `dtmm-core` resources |
| `crates/dtmm/src/controller/{app,worker}.rs`, `state/delegate.rs` | application adapters and action loop |
| `crates/dtmm/src/util/config.rs`, argument parsing in `main.rs` | application |
| `crates/dtmt/src/cmd/build.rs` deploy branch, `watch.rs` | core calls |

Notes:

- `app::load_initial` stays in the application for config loading, but the mod-store scan it
  performs should call the core store API.
- The GUI delegate becomes a consumer of deploy events instead of owning deployment state.

## Work items

### I0 - core crate and first move

- [ ] Create `lib/dtmm-core` and wire it into the workspace.
- [ ] Move detection, deploy, and reset with minimal interface changes; replace `ActionState` with
      `Context`.
- [ ] Rewrite the headless `--deploy`/`--reset` path as core calls.
- [ ] Replace druid collections (`Vector<Arc<...>>`) in moved code with plain collections.

### I1 - plan/apply and JSON

- [ ] Split `plan` from `apply`; make `DeploymentPlan` serializable.
- [ ] Add `Diagnostic` and `DeploymentReport`.
- [ ] Add subcommands `detect`, `status`, `list`, `plan`, `deploy`, `reset`.
- [ ] `--json` for all read and plan operations; `--dry-run` prints the plan without writing.
- [ ] Versioned deployment data with a writer field; game-directory lock.

### I2 - import/store and GUI adapter

- [ ] Move import/store; preserve archive validation and image extraction behavior.
- [ ] Events: `Reporter` trait or channel; the GUI renders progress from events.
- [ ] `dtmm import` and `dtmm list` end to end.

### I3 - dtmt consolidation and lean artifact

- [ ] `dtmt build --deploy` and `dtmt watch --deploy` call the core.
- [ ] Feature-gate druid; add a console binary target for command-line use if needed.

### I4 - documentation

- [ ] Library documentation and examples.
- [ ] CLI reference with JSON schemas and exit codes.
- [ ] Integration guide for other mod managers.

## Testing

- Core integration tests against fixture game directories (temporary copies with a fake `bundle/`
  and database): deploy, detect, plan idempotency, reset.
- Golden JSON for `status` and `plan`.
- Command-line tests: exit codes, `--dry-run` writes nothing, `--json` parses.
- Regression: GUI deploy/reset produce the same file state as the command line.

## Risks

- Moving code out of the GUI crate is a large mechanical refactor; type moves churn the UI layer.
- The public API is easiest to get wrong before the first extraction. `plan` in particular may
  need to expose more than the current deploy code tracks (for example, every file restore).
- Error handling: `eyre` is woven through the current controller code; typed errors need care to
  avoid losing context.
- Deployment data compatibility with existing installations.
- Windows console behavior and Linux GUI dependencies for headless use.
- Keeping the library free of GUI and config coupling while the application still needs both.

## Open decisions

- Library crate name and publishing target.
- Typed errors immediately, or keep `eyre` behind the core facade at first.
- Async API only, or an optional blocking facade for synchronous integrators.
- How much of import/store belongs in the core versus the application.
- Console handling: attach to the parent console, or a second binary target.
- Whether a later release removes the existing top-level flags in favor of subcommands only.
