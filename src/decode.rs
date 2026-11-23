use std::fmt;

pub const HEADER_SIZE: usize = 14;
pub const END_MARKER: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
const MAX_PIXELS: u64 = 400_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QoiError {
    HeaderTooShort,
    InvalidMagic,
    InvalidDimensions,
    InvalidChannels,
    InvalidColorspace,
    InvalidData(&'static str),
    ImageTooLarge,
    PixelBufferSize,
}

impl fmt::Display for QoiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderTooShort => write!(f, "QOI header is too short"),
            Self::InvalidMagic => write!(f, "invalid QOI file signature"),
            Self::InvalidDimensions => write!(f, "image dimensions must be nonzero"),
            Self::InvalidChannels => write!(f, "QOI channels must be 3 or 4"),
            Self::InvalidColorspace => write!(f, "QOI colorspace must be 0 or 1"),
            Self::InvalidData(message) => write!(f, "invalid QOI data: {message}"),
            Self::ImageTooLarge => write!(f, "QOI image exceeds the 400-million-pixel limit"),
            Self::PixelBufferSize => write!(f, "pixel buffer size does not match image dimensions"),
        }
    }
}

impl std::error::Error for QoiError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub colorspace: u8,
}

impl Header {
    pub fn pixel_count(self) -> Result<usize, QoiError> {
        let count = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .filter(|count| *count <= MAX_PIXELS)
            .ok_or(QoiError::ImageTooLarge)?;
        usize::try_from(count).map_err(|_| QoiError::ImageTooLarge)
    }

    pub fn byte_count(self) -> Result<usize, QoiError> {
        self.pixel_count()?
            .checked_mul(usize::from(self.channels))
            .ok_or(QoiError::ImageTooLarge)
    }
}

pub fn read_header(bytes: &[u8]) -> Result<Header, QoiError> {
    if bytes.len() < HEADER_SIZE {
        return Err(QoiError::HeaderTooShort);
    }
    if &bytes[..4] != b"qoif" {
        return Err(QoiError::InvalidMagic);
    }

    let header = Header {
        width: u32::from_be_bytes(bytes[4..8].try_into().expect("four-byte width")),
        height: u32::from_be_bytes(bytes[8..12].try_into().expect("four-byte height")),
        channels: bytes[12],
        colorspace: bytes[13],
    };
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
    Ok(header)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pixel(pub [u8; 4]);

impl Pixel {
    pub(crate) fn hash(self) -> usize {
        let channels = u32::from_ne_bytes(self.0) as u64;
        let packed = ((channels & 0xff00_ff00) << 32) | (channels & 0x00ff_00ff);
        (packed
            .wrapping_mul(0x0300_0700_0005_000b)
            .to_le()
            .swap_bytes() as u8
            & 63) as usize
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DecoderState {
    pub(crate) input_offset: usize,
    pub(crate) pixel_offset: usize,
    pub(crate) previous: Pixel,
    pub(crate) index: [Pixel; 64],
}

impl Default for DecoderState {
    fn default() -> Self {
        Self {
            input_offset: HEADER_SIZE,
            pixel_offset: 0,
            previous: Pixel([0, 0, 0, 255]),
            index: [Pixel([0, 0, 0, 0]); 64],
        }
    }
}

pub(crate) fn read_chunk(
    bytes: &[u8],
    state: &mut DecoderState,
) -> Result<(Pixel, usize), QoiError> {
    let tag = *bytes
        .get(state.input_offset)
        .ok_or(QoiError::InvalidData("missing pixel chunk"))?;
    state.input_offset += 1;
    let mut pixel = state.previous;
    let count;

    match tag {
        0xfe => {
            let rgb = bytes
                .get(state.input_offset..state.input_offset + 3)
                .ok_or(QoiError::InvalidData("truncated RGB chunk"))?;
            pixel.0[..3].copy_from_slice(rgb);
            state.input_offset += 3;
            count = 1;
        }
        0xff => {
            let rgba = bytes
                .get(state.input_offset..state.input_offset + 4)
                .ok_or(QoiError::InvalidData("truncated RGBA chunk"))?;
            pixel.0.copy_from_slice(rgba);
            state.input_offset += 4;
            count = 1;
        }
        _ => match tag >> 6 {
            0 => {
                pixel = state.index[usize::from(tag & 0x3f)];
                count = 1;
            }
            1 => {
                pixel.0[0] = pixel.0[0].wrapping_add(((tag >> 4) & 3).wrapping_sub(2));
                pixel.0[1] = pixel.0[1].wrapping_add(((tag >> 2) & 3).wrapping_sub(2));
                pixel.0[2] = pixel.0[2].wrapping_add((tag & 3).wrapping_sub(2));
                count = 1;
            }
            2 => {
                let second = *bytes
                    .get(state.input_offset)
                    .ok_or(QoiError::InvalidData("truncated luma chunk"))?;
                state.input_offset += 1;
                let green = i16::from(tag & 0x3f) - 32;
                pixel.0[0] = pixel.0[0].wrapping_add((green - 8 + i16::from(second >> 4)) as u8);
                pixel.0[1] = pixel.0[1].wrapping_add(green as u8);
                pixel.0[2] = pixel.0[2].wrapping_add((green - 8 + i16::from(second & 0x0f)) as u8);
                count = 1;
            }
            _ => {
                count = usize::from(tag & 0x3f) + 1;
            }
        },
    }

    let update_index = match tag {
        0..=0x3f => false,
        0xc0..=0xfd => state.pixel_offset == 0,
        _ => true,
    };
    if update_index {
        state.index[pixel.hash()] = pixel;
    }
    state.pixel_offset = state
        .pixel_offset
        .checked_add(count)
        .ok_or(QoiError::ImageTooLarge)?;
    state.previous = pixel;
    Ok((pixel, count))
}

pub(crate) fn decode_range(
    bytes: &[u8],
    header: Header,
    state: DecoderState,
    end_pixel: usize,
    output: &mut [u8],
) -> Result<DecoderState, QoiError> {
    let channels = usize::from(header.channels);
    let start_pixel = state.pixel_offset;
    if start_pixel > end_pixel
        || end_pixel > header.pixel_count()?
        || output.len() != (end_pixel - start_pixel) * channels
    {
        return Err(QoiError::InvalidData("invalid decode range"));
    }

    match header.channels {
        3 => decode_range_impl::<3>(bytes, state, end_pixel, output),
        4 => decode_range_impl::<4>(bytes, state, end_pixel, output),
        _ => Err(QoiError::InvalidChannels),
    }
}

fn decode_range_impl<const CHANNELS: usize>(
    bytes: &[u8],
    mut state: DecoderState,
    end_pixel: usize,
    output: &mut [u8],
) -> Result<DecoderState, QoiError> {
    let (mut pixels, remainder) = output.as_chunks_mut::<CHANNELS>();
    if !remainder.is_empty() {
        return Err(QoiError::InvalidData("invalid output buffer size"));
    }
    let mut input = bytes
        .get(state.input_offset..)
        .ok_or(QoiError::InvalidData("invalid input offset"))?;
    let mut pixel_offset = state.pixel_offset;

    while let [destination, tail @ ..] = pixels {
        pixels = tail;
        let mut pixel = state.previous;
        match input {
            [0xfe, red, green, blue, rest @ ..] => {
                pixel.0[..3].copy_from_slice(&[*red, *green, *blue]);
                input = rest;
            }
            [0xff, red, green, blue, alpha, rest @ ..] => {
                pixel.0.copy_from_slice(&[*red, *green, *blue, *alpha]);
                input = rest;
            }
            [tag @ 0..=0x3f, rest @ ..] => {
                pixel = state.index[usize::from(*tag)];
                input = rest;
                destination.copy_from_slice(&pixel.0[..CHANNELS]);
                state.previous = pixel;
                pixel_offset += 1;
                continue;
            }
            [tag @ 0x40..=0x7f, rest @ ..] => {
                pixel.0[0] = pixel.0[0].wrapping_add(((*tag >> 4) & 3).wrapping_sub(2));
                pixel.0[1] = pixel.0[1].wrapping_add(((*tag >> 2) & 3).wrapping_sub(2));
                pixel.0[2] = pixel.0[2].wrapping_add((*tag & 3).wrapping_sub(2));
                input = rest;
            }
            [tag @ 0x80..=0xbf, second, rest @ ..] => {
                let green = (*tag & 0x3f).wrapping_sub(32);
                pixel.0[0] =
                    pixel.0[0].wrapping_add(green.wrapping_sub(8).wrapping_add(*second >> 4));
                pixel.0[1] = pixel.0[1].wrapping_add(green);
                pixel.0[2] =
                    pixel.0[2].wrapping_add(green.wrapping_sub(8).wrapping_add(*second & 0x0f));
                input = rest;
            }
            [tag @ 0xc0..=0xfd, rest @ ..] => {
                let count = usize::from(*tag & 0x3f) + 1;
                if pixel_offset + count > end_pixel {
                    return Err(QoiError::InvalidData(
                        "run exceeds image dimensions or decode range",
                    ));
                }
                let mut output_pixel = [0; CHANNELS];
                output_pixel.copy_from_slice(&pixel.0[..CHANNELS]);
                destination.copy_from_slice(&output_pixel);
                let (fill, remaining) = pixels.split_at_mut(count - 1);
                fill.fill(output_pixel);
                pixels = remaining;
                input = rest;
                if pixel_offset == 0 {
                    state.index[pixel.hash()] = pixel;
                }
                state.previous = pixel;
                pixel_offset += count;
                continue;
            }
            _ => return Err(QoiError::InvalidData("truncated or invalid pixel chunk")),
        }

        destination.copy_from_slice(&pixel.0[..CHANNELS]);
        state.index[pixel.hash()] = pixel;
        state.previous = pixel;
        pixel_offset += 1;
    }

    state.input_offset = bytes.len() - input.len();
    state.pixel_offset = pixel_offset;
    Ok(state)
}

pub fn decode(bytes: &[u8]) -> Result<(Header, Vec<u8>), QoiError> {
    let header = read_header(bytes)?;
    if bytes.len() < HEADER_SIZE + END_MARKER.len() {
        return Err(QoiError::InvalidData(
            "file is shorter than the header and end marker",
        ));
    }
    let end = bytes.len() - END_MARKER.len();
    if bytes[end..] != END_MARKER {
        return Err(QoiError::InvalidData("invalid end marker"));
    }

    let mut output = vec![0; header.byte_count()?];
    let state = decode_range(
        bytes,
        header,
        DecoderState::default(),
        header.pixel_count()?,
        &mut output,
    )?;
    if state.input_offset != end {
        return Err(QoiError::InvalidData("unexpected data after final pixel"));
    }
    Ok((header, output))
}
