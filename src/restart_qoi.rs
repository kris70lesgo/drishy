//! An experimental QOI-derived stream with independently decodable restart blocks.
//!
//! The regular QOI format carries decoder state from one pixel to the next. This
//! format resets that state every `block_pixels` pixels, which lets a decoder
//! split the output buffer into disjoint ranges and decode them on CPU threads.

use crate::decode::{DecoderState, Header, Pixel, QoiError, decode_range};
use rayon::prelude::*;
use std::ops::Range;

const MAGIC: &[u8; 4] = b"DRQ1";
const FILE_HEADER_SIZE: usize = 22;
const MAX_BLOCK_COUNT: usize = 1_000_000;
/// Minimum pixel count before decoding restart blocks in parallel.
pub const PARALLEL_MIN_PIXELS: usize = 65_536;

/// Minimum pixel count before encoding restart blocks in parallel.
pub const PARALLEL_ENCODE_MIN_PIXELS: usize = 262_144;

/// Default number of row-major pixels in each independently decodable block.
pub const DEFAULT_BLOCK_PIXELS: usize = 16_384;

/// Image header, selected block size, and byte ranges for each compressed block.
struct ParsedStream {
    header: Header,
    block_pixels: usize,
    blocks: Vec<Range<usize>>,
}

/// Encodes RGB or RGBA pixels into the restart-block format using the default block size.
pub fn encode(header: Header, pixels: &[u8]) -> Result<Vec<u8>, QoiError> {
    encode_with_block_pixels(header, pixels, DEFAULT_BLOCK_PIXELS)
}

/// Encodes RGB or RGBA pixels into the restart-block format.
///
/// The format is experimental and is not compatible with `.qoi` files. Each
/// block uses QOI opcodes but starts with fresh previous-pixel and color-index
/// state. Use a smaller block size to expose more parallel work, or a larger one
/// to reduce restart overhead and keep more color history within each block.
pub fn encode_with_block_pixels(
    header: Header,
    pixels: &[u8],
    requested_block_pixels: usize,
) -> Result<Vec<u8>, QoiError> {
    validate_header(header)?;
    let total_pixels = header.pixel_count()?;
    if pixels.len() != header.byte_count()? {
        return Err(QoiError::PixelBufferSize);
    }
    if requested_block_pixels == 0 || requested_block_pixels > u32::MAX as usize {
        return Err(QoiError::InvalidData("invalid restart block size"));
    }

    let block_pixels = requested_block_pixels.min(total_pixels);
    let block_bytes = block_pixels
        .checked_mul(usize::from(header.channels))
        .ok_or(QoiError::ImageTooLarge)?;
    let block_count = total_pixels.div_ceil(block_pixels);
    if block_count > MAX_BLOCK_COUNT || block_count > u32::MAX as usize {
        return Err(QoiError::InvalidData("too many restart blocks"));
    }

    let directory_size = block_count.checked_mul(4).ok_or(QoiError::ImageTooLarge)?;
    let prefix_size = FILE_HEADER_SIZE
        .checked_add(directory_size)
        .ok_or(QoiError::ImageTooLarge)?;
    let mut output = Vec::with_capacity(prefix_size);
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&header.width.to_le_bytes());
    output.extend_from_slice(&header.height.to_le_bytes());
    output.push(header.channels);
    output.push(header.colorspace);
    output.extend_from_slice(&(block_pixels as u32).to_le_bytes());
    output.extend_from_slice(&(block_count as u32).to_le_bytes());
    output.resize(prefix_size, 0);

    // Reuse one bounded scratch area instead of allocating a temporary Vec for
    // every block. The length directory lets us append the finished blocks in
    // order after each batch has encoded in parallel.
    const MAX_BATCH_BLOCKS: usize = 64;
    let batch_blocks = block_count.min(MAX_BATCH_BLOCKS);
    let slot_size = block_bytes
        .checked_add(block_pixels)
        .ok_or(QoiError::ImageTooLarge)?;
    let scratch_size = batch_blocks
        .checked_mul(slot_size)
        .ok_or(QoiError::ImageTooLarge)?;
    let mut scratch = vec![0; scratch_size];
    let parallel = total_pixels >= PARALLEL_ENCODE_MIN_PIXELS && block_count > 1;

    for first_block in (0..block_count).step_by(MAX_BATCH_BLOCKS) {
        let count = (block_count - first_block).min(MAX_BATCH_BLOCKS);
        let first_pixel = first_block
            .checked_mul(block_pixels)
            .ok_or(QoiError::ImageTooLarge)?;
        let first_byte = first_pixel
            .checked_mul(usize::from(header.channels))
            .ok_or(QoiError::ImageTooLarge)?;
        let input_batch_bytes = count
            .checked_mul(block_bytes)
            .ok_or(QoiError::ImageTooLarge)?;
        let end_byte = first_byte + input_batch_bytes.min(pixels.len() - first_byte);
        let input_batch = &pixels[first_byte..end_byte];
        let scratch_batch_size = count
            .checked_mul(slot_size)
            .ok_or(QoiError::ImageTooLarge)?;
        let scratch_batch = &mut scratch[..scratch_batch_size];

        let lengths: Vec<usize> = if parallel {
            scratch_batch
                .par_chunks_mut(slot_size)
                .zip(input_batch.par_chunks(block_bytes))
                .map(|(slot, block)| {
                    let block_header = block_header(header, block)?;
                    crate::encode::encode_payload_into(block_header, block, slot)
                })
                .collect::<Result<_, _>>()?
        } else {
            scratch_batch
                .chunks_mut(slot_size)
                .zip(input_batch.chunks(block_bytes))
                .map(|(slot, block)| {
                    let block_header = block_header(header, block)?;
                    crate::encode::encode_payload_into(block_header, block, slot)
                })
                .collect::<Result<_, _>>()?
        };

        if first_block == 0 {
            // Use the already-encoded first batch as a sample to reserve close
            // to the final compressed size without running the encoder twice.
            let sample_payload = lengths.iter().try_fold(0_usize, |sum, length| {
                sum.checked_add(*length).ok_or(QoiError::ImageTooLarge)
            })?;
            let sample_pixels = input_batch.len() / usize::from(header.channels);
            let estimated_payload = usize::try_from(
                (sample_payload as u128 * total_pixels as u128) / sample_pixels as u128,
            )
            .map_err(|_| QoiError::ImageTooLarge)?;
            let reserve = estimated_payload
                .checked_add(estimated_payload / 8)
                .ok_or(QoiError::ImageTooLarge)?;
            output.reserve(reserve);
        }

        for (local_block, (&length, slot)) in lengths
            .iter()
            .zip(scratch_batch.chunks(slot_size))
            .enumerate()
        {
            let length_u32 = u32::try_from(length)
                .map_err(|_| QoiError::InvalidData("restart block is too large"))?;
            let directory_entry = FILE_HEADER_SIZE + (first_block + local_block) * 4;
            output[directory_entry..directory_entry + 4].copy_from_slice(&length_u32.to_le_bytes());
            output.extend_from_slice(&slot[..length]);
        }
    }
    Ok(output)
}

fn block_header(header: Header, pixels: &[u8]) -> Result<Header, QoiError> {
    let count = pixels.len() / usize::from(header.channels);
    Ok(Header {
        width: u32::try_from(count).map_err(|_| QoiError::ImageTooLarge)?,
        height: 1,
        channels: header.channels,
        colorspace: header.colorspace,
    })
}

/// Encodes a tightly packed RGB or RGBA image into the restart-block format.
pub fn encode_rgb_or_rgba(
    width: u32,
    height: u32,
    channels: u8,
    pixels: &[u8],
) -> Result<Vec<u8>, QoiError> {
    encode(
        Header {
            width,
            height,
            channels,
            colorspace: 0,
        },
        pixels,
    )
}

/// Reads and validates the restart-block file header and length directory.
pub fn read_header(bytes: &[u8]) -> Result<Header, QoiError> {
    Ok(parse_stream(bytes)?.header)
}

/// Decodes the image into a newly allocated RGB or RGBA pixel buffer.
pub fn decode(bytes: &[u8]) -> Result<(Header, Vec<u8>), QoiError> {
    let stream = parse_stream(bytes)?;
    let header = stream.header;
    let mut pixels = vec![0; header.byte_count()?];
    decode_parsed_into(bytes, &stream, &mut pixels, None)?;
    Ok((header, pixels))
}

/// Decodes the image into a caller-owned RGB or RGBA pixel buffer.
pub fn decode_into(bytes: &[u8], output: &mut [u8]) -> Result<Header, QoiError> {
    decode_into_impl(bytes, output, None)
}

/// Decodes into a caller-owned buffer using a reusable Rayon thread pool.
pub fn decode_into_with_pool(
    bytes: &[u8],
    output: &mut [u8],
    pool: &rayon::ThreadPool,
) -> Result<Header, QoiError> {
    decode_into_impl(bytes, output, Some(pool))
}

/// Decodes with a reusable Rayon thread pool and allocates the output buffer.
pub fn decode_with_pool(
    bytes: &[u8],
    pool: &rayon::ThreadPool,
) -> Result<(Header, Vec<u8>), QoiError> {
    let stream = parse_stream(bytes)?;
    let header = stream.header;
    let mut pixels = vec![0; header.byte_count()?];
    decode_parsed_into(bytes, &stream, &mut pixels, Some(pool))?;
    Ok((header, pixels))
}

fn validate_header(header: Header) -> Result<(), QoiError> {
    if header.width == 0 || header.height == 0 {
        return Err(QoiError::InvalidDimensions);
    }
    if header.channels != 3 && header.channels != 4 {
        return Err(QoiError::InvalidChannels);
    }
    if header.colorspace > 1 {
        return Err(QoiError::InvalidColorspace);
    }
    header.pixel_count()?;
    Ok(())
}

fn parse_stream(bytes: &[u8]) -> Result<ParsedStream, QoiError> {
    if bytes.len() < FILE_HEADER_SIZE {
        return Err(QoiError::HeaderTooShort);
    }
    if &bytes[..4] != MAGIC {
        return Err(QoiError::InvalidMagic);
    }
    let header = Header {
        width: u32::from_le_bytes(bytes[4..8].try_into().expect("four-byte width")),
        height: u32::from_le_bytes(bytes[8..12].try_into().expect("four-byte height")),
        channels: bytes[12],
        colorspace: bytes[13],
    };
    validate_header(header)?;
    let total_pixels = header.pixel_count()?;
    let stored_block_pixels =
        u32::from_le_bytes(bytes[14..18].try_into().expect("four-byte block size"));
    if stored_block_pixels == 0 {
        return Err(QoiError::InvalidData("restart block size is zero"));
    }
    let block_pixels = (stored_block_pixels as usize).min(total_pixels);
    let stored_block_count =
        u32::from_le_bytes(bytes[18..22].try_into().expect("four-byte block count")) as usize;
    let expected_block_count = total_pixels.div_ceil(block_pixels);
    if stored_block_count != expected_block_count || stored_block_count > MAX_BLOCK_COUNT {
        return Err(QoiError::InvalidData("restart block count is invalid"));
    }
    let directory_size = stored_block_count
        .checked_mul(4)
        .ok_or(QoiError::ImageTooLarge)?;
    let mut payload_offset = FILE_HEADER_SIZE
        .checked_add(directory_size)
        .filter(|end| *end <= bytes.len())
        .ok_or(QoiError::InvalidData("truncated restart block directory"))?;

    let mut blocks = Vec::with_capacity(stored_block_count);
    for entry in bytes[FILE_HEADER_SIZE..FILE_HEADER_SIZE + directory_size].chunks_exact(4) {
        let length = u32::from_le_bytes(entry.try_into().expect("four-byte block length")) as usize;
        if length == 0 {
            return Err(QoiError::InvalidData("restart block is empty"));
        }
        let end = payload_offset
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or(QoiError::InvalidData("truncated restart block"))?;
        blocks.push(payload_offset..end);
        payload_offset = end;
    }
    if payload_offset != bytes.len() {
        return Err(QoiError::InvalidData(
            "unexpected data after restart blocks",
        ));
    }

    Ok(ParsedStream {
        header,
        block_pixels,
        blocks,
    })
}

fn decode_into_impl(
    bytes: &[u8],
    output: &mut [u8],
    pool: Option<&rayon::ThreadPool>,
) -> Result<Header, QoiError> {
    let stream = parse_stream(bytes)?;
    decode_parsed_into(bytes, &stream, output, pool)
}

fn decode_parsed_into(
    bytes: &[u8],
    stream: &ParsedStream,
    output: &mut [u8],
    pool: Option<&rayon::ThreadPool>,
) -> Result<Header, QoiError> {
    let header = stream.header;
    if output.len() != header.byte_count()? {
        return Err(QoiError::PixelBufferSize);
    }

    let channels = usize::from(header.channels);
    let block_bytes = stream
        .block_pixels
        .checked_mul(channels)
        .ok_or(QoiError::ImageTooLarge)?;
    let block_count = stream.blocks.len();
    if block_count == 1 || header.pixel_count()? < PARALLEL_MIN_PIXELS {
        decode_blocks_sequential(bytes, header, stream.block_pixels, &stream.blocks, output)?;
    } else if let Some(pool) = pool {
        pool.install(|| {
            decode_blocks_parallel(
                bytes,
                header,
                stream.block_pixels,
                &stream.blocks,
                block_bytes,
                output,
            )
        })?;
    } else {
        decode_blocks_parallel(
            bytes,
            header,
            stream.block_pixels,
            &stream.blocks,
            block_bytes,
            output,
        )?;
    }
    Ok(header)
}

fn decode_blocks_sequential(
    bytes: &[u8],
    header: Header,
    block_pixels: usize,
    blocks: &[Range<usize>],
    output: &mut [u8],
) -> Result<(), QoiError> {
    let block_bytes = block_pixels
        .checked_mul(usize::from(header.channels))
        .ok_or(QoiError::ImageTooLarge)?;
    for (position, (range, destination)) in blocks
        .iter()
        .zip(output.chunks_mut(block_bytes))
        .enumerate()
    {
        decode_block(bytes, header, block_pixels, position, range, destination)?;
    }
    Ok(())
}

fn decode_blocks_parallel(
    bytes: &[u8],
    header: Header,
    block_pixels: usize,
    blocks: &[Range<usize>],
    block_bytes: usize,
    output: &mut [u8],
) -> Result<(), QoiError> {
    output
        .par_chunks_mut(block_bytes)
        .zip(blocks.par_iter())
        .enumerate()
        .try_for_each(|(position, (destination, range))| {
            decode_block(bytes, header, block_pixels, position, range, destination)
        })
}

fn decode_block(
    bytes: &[u8],
    image_header: Header,
    block_pixels: usize,
    position: usize,
    range: &Range<usize>,
    output: &mut [u8],
) -> Result<(), QoiError> {
    let first_pixel = position
        .checked_mul(block_pixels)
        .ok_or(QoiError::ImageTooLarge)?;
    let count = (image_header.pixel_count()? - first_pixel).min(block_pixels);
    let block_header = Header {
        width: u32::try_from(count).map_err(|_| QoiError::ImageTooLarge)?,
        height: 1,
        channels: image_header.channels,
        colorspace: image_header.colorspace,
    };
    let state = DecoderState {
        input_offset: 0,
        pixel_offset: 0,
        previous: Pixel([0, 0, 0, 255]),
        index: [Pixel([0, 0, 0, 0]); 64],
    };
    let payload = &bytes[range.clone()];
    let final_state = decode_range(payload, block_header, state, count, output)?;
    if final_state.input_offset != payload.len() {
        return Err(QoiError::InvalidData("unused bytes in restart block"));
    }
    Ok(())
}
