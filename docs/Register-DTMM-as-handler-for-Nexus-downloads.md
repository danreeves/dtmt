Nexusmods uses a custom link style (i.e. a custom URI, `nxm://` instead of `https://`) to connect the "Mod Manager Download" button in the browser with the application running on your machine.
The connection between those two, and the way for a user to select the application (e.g. Vortex, MO2, DTMM) is through a so-called "URI handler".

In Windows, these are stored in the Registry. Usually, the typical "setup.exe" installers will create them for you, but DTMM isn't quite there yet, so for now, you will have to do it with a script or manually.
See [learn.microsoft.com](https://learn.microsoft.com/en-us/previous-versions/windows/internet-explorer/ie-developer/platform-apis/aa767914(v=vs.85)?redirectedfrom=MSDN#app_reg) for official details how this works.

**The recommended approach is to set up NXM Proxy from https://www.nexusmods.com/site/mods/288**.

## NXM Proxy

Windows itself only allows one application to be designated as handler for the `nxm://` URI, which means that only one mod manager would have to handle all Nexus-compatible games. The purpose of NXM Proxy is to act as a broker, which is registered as that designated handler, checks the game id in the URI and forwards it to other applications based on configuration parameters.

1. Install NXM Proxy for Vortex from https://www.nexusmods.com/site/mods/288 and make sure its enabled.
2. Add DTMM as a new manager for NXM Proxy. The command line should look like `"C:\path\to\dtmm.exe" %1` (**adjust the path to point to where you saved `dtmm.exe`**).
3. Select `Darktide` in the list of games, and map DTMM as its manager

## Script

This will set DTMM as the designated URI handler. You probably don't want this if you use Nexus for more games than just Darktide.

1. Copy the following content into _Notepad_ or a similar text editor
2. **Adjust the paths to point to where you saved `dtmm.exe`**
3. Save the file as `dtmm_uri_handler.reg` (make sure to set the file type to "All files (*.*)").
4. Double-click the file

```reg
Windows Registry Editor Version 5.00

[HKEY_CLASSES_ROOT\nxm]
"URL Protocol"=""
@="URL:Nexusmods Mod Manager"
"DefaultIcon"="\"C:\\path\\to\\dtmm.exe\",1"

[HKEY_CLASSES_ROOT\nxm\shell]

[HKEY_CLASSES_ROOT\nxm\shell\open]

[HKEY_CLASSES_ROOT\nxm\shell\open\command]
@="\"C:\\path\\to\\dtmm.exe\" \"%1\""
```

**Why copy the text instead of downloading the file?**

You would need to edit it anyways, to adjust the file paths. But more importantly, for your own security.
For one, this shows you the entire content for review before you even have to touch it. For two, Windows applies different policies to files that were downloaded as-is compared to the ones you create yourself. So by saving it yourself, you avoid any extra "Have you verified that this is safe?" dialogs.

## Manual

Open `regedit.exe` and perform the steps described in the script above.