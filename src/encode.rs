use crate::decode::{END_MARKER, Header, Pixel, QoiError, read_header};

pub fn encode(header: Header, pixels: &[u8]) -> Result<Vec<u8>, QoiError> {
    let mut raw_header = Vec::with_capacity(14);
    raw_header.extend_from_slice(b"qoif");
    raw_header.extend_from_slice(&header.width.to_be_bytes());
    raw_header.extend_from_slice(&header.height.to_be_bytes());
    raw_header.push(header.channels);
    raw_header.push(header.colorspace);
    let header = read_header(&raw_header)?;
    if pixels.len() != header.byte_count()? {
        return Err(QoiError::PixelBufferSize);
    }

    let mut output = Vec::with_capacity(pixels.len() + pixels.len() / 2 + 22);
    output.extend_from_slice(b"qoif");
    output.extend_from_slice(&header.width.to_be_bytes());
    output.extend_from_slice(&header.height.to_be_bytes());
    output.push(header.channels);
    output.push(header.colorspace);

    encode_payload(header, pixels, &mut output)?;
    output.extend_from_slice(&END_MARKER);
    Ok(output)
}

/// Appends a QOI pixel stream without a file header or end marker.
pub(crate) fn encode_payload(
    header: Header,
    pixels: &[u8],
    output: &mut Vec<u8>,
) -> Result<(), QoiError> {
    encode_payload_to(header, pixels, output)
}

pub(crate) fn encode_payload_into(
    header: Header,
    pixels: &[u8],
    output: &mut [u8],
) -> Result<usize, QoiError> {
    let mut writer = SliceWriter { output, written: 0 };
    encode_payload_to(header, pixels, &mut writer)?;
    Ok(writer.written)
}

trait QoiOutput {
    fn push_byte(&mut self, byte: u8) -> Result<(), QoiError>;
    fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), QoiError>;
}

impl QoiOutput for Vec<u8> {
    #[inline]
    fn push_byte(&mut self, byte: u8) -> Result<(), QoiError> {
        self.push(byte);
        Ok(())
    }

    #[inline]
    fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), QoiError> {
        self.extend_from_slice(bytes);
        Ok(())
    }
}

struct SliceWriter<'a> {
    output: &'a mut [u8],
    written: usize,
}

impl QoiOutput for SliceWriter<'_> {
    #[inline]
    fn push_byte(&mut self, byte: u8) -> Result<(), QoiError> {
        let Some(slot) = self.output.get_mut(self.written) else {
            return Err(QoiError::InvalidData("restart block output is too small"));
        };
        *slot = byte;
        self.written += 1;
        Ok(())
    }

    #[inline]
    fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), QoiError> {
        let end = self
            .written
            .checked_add(bytes.len())
            .ok_or(QoiError::ImageTooLarge)?;
        let Some(destination) = self.output.get_mut(self.written..end) else {
            return Err(QoiError::InvalidData("restart block output is too small"));
        };
        destination.copy_from_slice(bytes);
        self.written = end;
        Ok(())
    }
}

fn encode_payload_to<W: QoiOutput>(
    header: Header,
    pixels: &[u8],
    output: &mut W,
) -> Result<(), QoiError> {
    if pixels.len() != header.byte_count()? {
        return Err(QoiError::PixelBufferSize);
    }

    let channels = usize::from(header.channels);
    let mut previous = Pixel([0, 0, 0, 255]);
    let mut index = [Pixel([0, 0, 0, 0]); 64];
    let mut run = 0u8;

    let total_pixels = header.pixel_count()?;
    for (pixel_number, pixel_bytes) in pixels.chunks_exact(channels).enumerate() {
        let mut current = previous;
        current.0[..channels].copy_from_slice(pixel_bytes);
        if current == previous {
            run += 1;
            if run == 62 || pixel_number + 1 == total_pixels {
                output.push_byte(0xc0 | (run - 1))?;
                run = 0;
            }
            continue;
        }

        if run != 0 {
            output.push_byte(0xc0 | (run - 1))?;
            run = 0;
        }

        let slot = current.hash();
        if index[slot] == current {
            output.push_byte(slot as u8)?;
        } else {
            index[slot] = current;
            if current.0[3] == previous.0[3] {
                let red = i16::from(current.0[0].wrapping_sub(previous.0[0]) as i8);
                let green = i16::from(current.0[1].wrapping_sub(previous.0[1]) as i8);
                let blue = i16::from(current.0[2].wrapping_sub(previous.0[2]) as i8);
                let red_green = red - green;
                let blue_green = blue - green;
                if (-2..=1).contains(&red) && (-2..=1).contains(&green) && (-2..=1).contains(&blue)
                {
                    output.push_byte(
                        0x40 | (((red + 2) as u8) << 4)
                            | (((green + 2) as u8) << 2)
                            | (blue + 2) as u8,
                    )?;
                } else if (-32..=31).contains(&green)
                    && (-8..=7).contains(&red_green)
                    && (-8..=7).contains(&blue_green)
                {
                    output.push_byte(0x80 | (green + 32) as u8)?;
                    output.push_byte(((red_green + 8) as u8) << 4 | (blue_green + 8) as u8)?;
                } else {
                    output.push_byte(0xfe)?;
                    output.push_bytes(&current.0[..3])?;
                }
            } else {
                output.push_byte(0xff)?;
                output.push_bytes(&current.0)?;
            }
        }
        previous = current;
    }

    if run != 0 {
        output.push_byte(0xc0 | (run - 1))?;
    }
    Ok(())
}

pub fn encode_rgb_or_rgba(
    width: u32,
    height: u32,
    channels: u8,
    pixels: &[u8],
) -> Result<Vec<u8>, QoiError> {
    // Reuse header validation so the encoder and decoder enforce the same format limits.
    let mut bytes = Vec::with_capacity(14);
    bytes.extend_from_slice(b"qoif");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.push(channels);
    bytes.push(0);
    let validated = read_header(&bytes)?;
    encode(validated, pixels)
}
