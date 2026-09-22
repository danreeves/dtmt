## General

There seem to be two ways that a texture can be stored in a bundle "data file" vs "stream file".

- "data file":
  - points to a file in `data/` without `.stream` extension
  - size of the file in `data/` can range from 200 bytes to several MB -> no apparent correlation
  - `external == true`, no data in bundle
- "stream file":
  - points to a `.stream` file in `data/`
  - data file size can range from 90 bytes to several MB
  - always contains data in the bundle file, size ranging from 200 bytes to several KB

In the case of stream files, the bundle will contain metadata (e.g. DDS headers) and a small mipmap, while the data file will contain the other mipmaps.

Other things of note:

- `BundleFileHeader.unknown_1` is always `1`
- textures can be duplicated across bundles

### Formats

Formats observed so far, all via a `DX10` FourCC:

| FourCC | DXGI |
|---------|-------|
| `DX10` | `BC1_UNORM` (71) |
| `DX10` | `BC4_UNORM` (80) |
| `DX10` | `BC5_UNORM` (83) |
| `DX10` | `BC7_UNORM` (98) |

The initial sample only contained `BC5_UNORM`; the list above comes from a
wider scan of `bundle/data/`. See
[File Type - Texture.-](File%20Type%20-%20Texture.-.md) for the full layout and
the stream chunking rules.


## Decompiling

As an initial implementation, decompilation only extracts the largest (i.e. first) mipmap from streamed content.