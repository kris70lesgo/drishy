# DRQ1 format

DRQ1 is Drishy's experimental restart-block image format. It uses QOI pixel opcodes inside independent blocks, but a DRQ1 file is not a standard QOI file. This document describes the version implemented by this repository; the format is experimental and may change.

All multi-byte integers are unsigned little-endian values. The file contains a 22-byte header, a block-length directory, and the block payloads in order:

| Offset | Size | Field | Meaning |
| ---: | ---: | --- | --- |
| 0 | 4 | Magic | ASCII `DRQ1` |
| 4 | 4 | Width | Image width in pixels; nonzero |
| 8 | 4 | Height | Image height in pixels; nonzero |
| 12 | 1 | Channels | `3` for RGB or `4` for RGBA |
| 13 | 1 | Colorspace | `0` for sRGB with linear alpha, `1` for all-linear channels |
| 14 | 4 | Block pixels | Nonzero maximum pixel count in each block |
| 18 | 4 | Block count | Number of directory entries and payloads |
| 22 | `4 × block count` | Length directory | One nonzero byte length per payload, in block order |
| after directory | variable | Payloads | Concatenated block byte streams, in block order |

## Block layout and decoding

Pixels are ordered left-to-right, then top-to-bottom, as one row-major sequence. Block `i` starts at pixel `i × block_pixels` and contains up to `block_pixels` pixels. The last block may be shorter. The expected block count is the ceiling of `width × height / block_pixels`.

Each payload contains QOI opcodes for exactly that block's pixels. It does not include a QOI header or QOI end marker. Decoding state resets at every block:

- Previous pixel: `[0, 0, 0, 255]`.
- Each of the 64 QOI color-index entries: `[0, 0, 0, 0]`.

Because state resets, each block can be decoded independently. A run opcode must fit entirely within its block. The length directory gives each payload's exact byte length; a decoder rejects empty or truncated payloads, a wrong block count, and bytes left over after the final payload.

The implementation validates nonzero dimensions, RGB/RGBA channels, and colorspace values `0` or `1`. It caps images at 400 million pixels and block counts at 1,000,000 as implementation safety limits. Encoders use a default block size of 16,384 pixels. The current decoder starts parallel work at 65,536 pixels; these are implementation settings, not format requirements.

For standard QOI's opcode meanings, see the [QOI specification](https://qoiformat.org/qoi-specification.pdf). DRQ1 payloads reuse those opcodes with the independent block state defined above.
