# Installing mods

1. Ensure that the "Data Directory" and "Game Directory" in the "Settings" tab are set properly.
2. Download the mod's release archive (a `.zip` file).
3. Click "Add mod" in DTMM and select the file you downloaded.
4. Use the checkbox in the mod list to enable the new mod.
5. Click "Deploy mods" and watch the log at the bottom until it completed.
6. Start the game normally.

## Handle game updates

After the game was updated, mods need to be deployed again. Simply click "Deploy mods" and watch the log view at the bottom, as usual.

## Migrating from dtkit-patch

_NOTE: This includes installations managed manually or with Vortex._

1. Copy the `mod_load_order.txt` file from the game files somewhere else, for reference later
2. Click "Reset Game"
3. Download the newest versions of your mods and install them, following the steps above

# Troubleshooting

If you've run into issues or received an error message during installation or game deployment, check the sections below.
If those didn't help, you'll always find helpful people in the [Modders Discord](https://discord.gg/GFyCGNpJb8).

When reporting issues, please attach a screenshot of the error message and DTMM's log file. You can find the log by entering `%APPDATA%\dtmm` into the File Explorer's address bar.

## "Found dtkit-patch-based mod installation"

Follow the migration steps above.

## "Failed to open game bundle directory"

DTMM needs to know where to find the game files. Double-check the "Game Directory" value in the "Settings" tab, and make sure it points to the game's installation directory, e.g. `E:\SteamLibrary\steamapps\common\Warhammer 40000 Darktide`.

## "Failed to import mod"

DTMM requires the mod archives to be built in a defined way. If importing fails for a mod, it might still only support the old ecosystem.
Check if there is a newer version. If not, try to get in touch with the author and point them to [Migrating from loose files](Migrating-from-loose-files) and [Building your first mod](Building-your-first-mod). The [Modders Discord](https://discord.gg/GFyCGNpJb8) is a good way to get in touch with the authors, or other people that can help you out.

## Mod dependency issues

> "Dependency XXX must be enabled"<br>
> "Dependency XXX must be loaded before YYY"<br>
> "Dependency XXX must be loaded after YYY"<br>

One of your mods specified another mod as dependency, optionally with a required order. Check the error message, and the mod's description and adjust the order accordingly, with the "Move Up" and "Move Down" buttons.

