# File Type - Lua

A `lua` (`0xa14e8dfa2cd117e2`) is a Lua script or module. The game wraps each one in a **24-byte header** and
stores either plain source or LuaJIT bytecode inside it.

## The wrapper

```text
0   u32   always 0
4   u32   the payload's length
8   [16]  the rest of the header
24  [u8]  the payload
```

The payload's length is checked against the resource's size, and the payload is either:

- **LuaJIT bytecode**, identified by the magic `0x8253461B` at its first word, or
- **plain source**, which is written out as-is.

The distinction matters because the two take different paths: source is passed through untouched,
while bytecode goes through LuaJIT for both directions.

## Decompilation and compilation

`dtmt` reads the wrapper, hands bytecode to LuaJIT (the `luajit2-sys` dependency, so it is in
process), and writes out source. Compiling goes back the other way and re-wraps the result with the
24-byte header, deriving the length word.

## Open

The file format itself is done. What is not properly implemented is the interaction with **bundle
properties and file variants** - see [issue #3](https://git.sclu1034.dev/bitsquid_dt/dtmt/issues/3).
