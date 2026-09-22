Presumably the overall structure will still be similar to VT2.

## Shaders

Shaders are compiled into DirectX Bytecode (can be found by the `DXBC` magic header).
Attempting to decompile the shader code back into somewhat usable syntax would probably be a massive undertaking. A better approach for the beginning would be to to extract each shader in its compiled form, and reference them by a hash.
This way, we could build a repository of all existing code blobs, slowly fill it with meta information as we figure that out (e.g. what each blob does, what variables it uses, etc.) and all tools could pull from that repository. E.g. the `.material` SJSON would have a special syntax to reference these code blobs, such that when DTMT is used to compile that material, it would know to pull that shader blob. Decompilation could then produce that syntax, making idempotent re-compiling and in-place edits possible.