> To be fleshed out, for now this is just me dumping steps as I do them

> Currently stuck at the fact that breakpoints don't seem to work

## Basic setup

1. Open the `darktide_launcher.log` and find the arguments to run the executable with.

    At the time of writing: `-eac-untrusted --bundle-dir ../bundle --ini settings --backend-auth-service-url https://bsp-auth-prod.atoma.cloud --backend-title-service-url https://bsp-td-prod.atoma.cloud -game -launcher_verification_passed_crashify_property false`
    
2. Find the necessary env variables and commands to run Proton from CLI.
    At the time of writing:

```bash
STEAM_COMPAT_CLIENT_INSTALL_PATH=/usr/bin/steam
# `1361210` is the Steam AppID for Darktide. It's in the same library as the game, so through "Browse files" in the UI works as starting point
STEAM_COMPAT_DATA_PATH=/path/to/SteamLibrary/steamapps/compatdata/1361210
# Any Proton version could be used, if it works with the game.
# And it's fine if this is installed in a different `SteamLibrary` than the game
/path/to/SteamLibrary/steamapps/common/Proton\ 8.0/proton run <cmd>
```

3. Add `PROTON_DUMP_DEBUG_COMMANDS=1`, then look in `/tmp/proton_$(whoami)`
4. Copy `gdb_run`, and alter the `winedbg` command with `s/--dbg/--dbg --no-start --port 2159/` (or a port of your choosing).
    Worthwhile to have it as separate file `gdb_start`, so that `gdb_run` stays available.

    `--no-start` will make gdb wait for the frontend to send the first `continue` command, which gives time to attach Ghidra and have it set up its breakpoints

5. Run the script in a terminal, it will echo `target remote <host>:<port>`
6. In Ghidra, for the debugger configuration, choose "remote gdb", enter the port and run. Breakpoints should become available
7. Run `continue` (or just `c`) in the gdb terminal to start the game, or any other gdb command.

## Observations

- Ghidra doesn't seem to set breakpoints automatically. Instead, the "Debug Console" window will talk about "there are breakpoints to place" and a button to do that
- multi-process mode, e.g. when Ghidra attaches as `extended-remote`, or the gdbserver is started with `--multi`, should allow restarting the