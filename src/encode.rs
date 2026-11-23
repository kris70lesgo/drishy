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

    let channels = usize::from(header.channels);
    let mut previous = Pixel([0, 0, 0, 255]);
    let mut index = [Pixel([0, 0, 0, 0]); 64];
    let mut run = 0u8;

    let total_pixels = header.pixel_count()?;
    for (pixel_number, pixel_bytes) in pixels.chunks_exact(channels).enumerate() {
        let mut current = previous;
        current.0[..channels].copy_from_slice(pixel_bytes);
        if current == previous {
            index[current.hash()] = current;
            run += 1;
            if run == 62 || pixel_number + 1 == total_pixels {
                output.push(0xc0 | (run - 1));
                run = 0;
            }
            continue;
        }

        if run != 0 {
            output.push(0xc0 | (run - 1));
            run = 0;
        }

        let slot = current.hash();
        if index[slot] == current {
            output.push(slot as u8);
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
                    output.push(
                        0x40 | (((red + 2) as u8) << 4)
                            | (((green + 2) as u8) << 2)
                            | (blue + 2) as u8,
                    );
                } else if (-32..=31).contains(&green)
                    && (-8..=7).contains(&red_green)
                    && (-8..=7).contains(&blue_green)
                {
                    output.push(0x80 | (green + 32) as u8);
                    output.push(((red_green + 8) as u8) << 4 | (blue_green + 8) as u8);
                } else {
                    output.push(0xfe);
                    output.extend_from_slice(&current.0[..3]);
                }
            } else {
                output.push(0xff);
                output.extend_from_slice(&current.0);
            }
        }
        previous = current;
    }

    if run != 0 {
        output.push(0xc0 | (run - 1));
    }
    output.extend_from_slice(&END_MARKER);
    Ok(output)
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
