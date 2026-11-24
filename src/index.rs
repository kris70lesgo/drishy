use crate::decode::{DecoderState, Header, Pixel, QoiError, read_chunk, read_header};
use std::io::{self, Read, Write};

const INDEX_MAGIC: &[u8; 8] = b"DRSHYIDX";
const INDEX_VERSION: u32 = 1;
const MAX_CHECKPOINTS: usize = 1_024;

#[derive(Debug, Clone)]
pub struct DecodeIndex {
    pub(crate) file_size: u64,
    pub(crate) fingerprint: u64,
    pub(crate) checkpoints: Vec<DecoderState>,
}

impl DecodeIndex {
    pub fn build(qoi: &[u8], workers: usize) -> Result<Self, QoiError> {
        let header = read_header(qoi)?;
        if workers == 0 {
            return Err(QoiError::InvalidData("worker count must be at least one"));
        }
        if workers > MAX_CHECKPOINTS {
            return Err(QoiError::InvalidData("too many decode workers"));
        }
        if qoi.len() < 22 || qoi[qoi.len() - 8..] != crate::decode::END_MARKER {
            return Err(QoiError::InvalidData("missing valid QOI end marker"));
        }

        let pixel_count = header.pixel_count()?;
        let mut checkpoints = vec![DecoderState::default()];
        let mut state = DecoderState::default();
        let mut next_boundary = pixel_count.div_ceil(workers);
        let data_end = qoi.len() - 8;
        let mut fingerprint = fingerprint(&qoi[..crate::decode::HEADER_SIZE]);
        while state.pixel_offset < pixel_count {
            if state.input_offset >= data_end {
                return Err(QoiError::InvalidData("pixel data ends early"));
            }
            let chunk_start = state.input_offset;
            read_chunk(qoi, &mut state)?;
            fingerprint = fingerprint_update(fingerprint, &qoi[chunk_start..state.input_offset]);
            if state.pixel_offset > pixel_count {
                return Err(QoiError::InvalidData("run exceeds image dimensions"));
            }
            if checkpoints.len() < workers
                && state.pixel_offset < pixel_count
                && state.pixel_offset >= next_boundary
            {
                checkpoints.push(state.clone());
                next_boundary = pixel_count
                    .saturating_mul(checkpoints.len() + 1)
                    .div_ceil(workers);
            }
        }
        if state.input_offset != data_end {
            return Err(QoiError::InvalidData("unexpected data after final pixel"));
        }
        fingerprint = fingerprint_update(fingerprint, &qoi[data_end..]);

        Ok(Self {
            file_size: qoi.len() as u64,
            fingerprint,
            checkpoints,
        })
    }

    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }

    pub fn write_to<W: Write>(&self, mut writer: W) -> io::Result<()> {
        writer.write_all(INDEX_MAGIC)?;
        writer.write_all(&INDEX_VERSION.to_le_bytes())?;
        writer.write_all(&self.file_size.to_le_bytes())?;
        writer.write_all(&self.fingerprint.to_le_bytes())?;
        writer.write_all(&(self.checkpoints.len() as u32).to_le_bytes())?;
        for checkpoint in &self.checkpoints {
            writer.write_all(&(checkpoint.input_offset as u64).to_le_bytes())?;
            writer.write_all(&(checkpoint.pixel_offset as u64).to_le_bytes())?;
            writer.write_all(&checkpoint.previous.0)?;
            for pixel in checkpoint.index {
                writer.write_all(&pixel.0)?;
            }
        }
        Ok(())
    }

    pub fn read_from<R: Read>(mut reader: R) -> io::Result<Self> {
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        if &magic != INDEX_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid index signature",
            ));
        }
        let version = read_u32(&mut reader)?;
        if version != INDEX_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported index version",
            ));
        }
        let file_size = read_u64(&mut reader)?;
        let fingerprint = read_u64(&mut reader)?;
        let count = read_u32(&mut reader)? as usize;
        if count == 0 || count > MAX_CHECKPOINTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid checkpoint count",
            ));
        }
        let mut checkpoints = Vec::with_capacity(count);
        for _ in 0..count {
            let input_offset = usize::try_from(read_u64(&mut reader)?).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "input offset too large")
            })?;
            let pixel_offset = usize::try_from(read_u64(&mut reader)?).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "pixel offset too large")
            })?;
            let mut previous = [0; 4];
            reader.read_exact(&mut previous)?;
            let mut index = [Pixel([0; 4]); 64];
            for pixel in &mut index {
                reader.read_exact(&mut pixel.0)?;
            }
            checkpoints.push(DecoderState {
                input_offset,
                pixel_offset,
                previous: Pixel(previous),
                index,
            });
        }
        Ok(Self {
            file_size,
            fingerprint,
            checkpoints,
        })
    }

    pub(crate) fn validate(&self, bytes: &[u8]) -> Result<Header, QoiError> {
        if bytes.len() as u64 != self.file_size || fingerprint(bytes) != self.fingerprint {
            return Err(QoiError::InvalidData("index does not match this QOI file"));
        }
        let header = read_header(bytes)?;
        let total_pixels = header.pixel_count()?;
        let data_end = bytes.len().saturating_sub(8);
        let first = self
            .checkpoints
            .first()
            .ok_or(QoiError::InvalidData("index has no checkpoints"))?;
        if first.pixel_offset != 0 || first.input_offset != crate::decode::HEADER_SIZE {
            return Err(QoiError::InvalidData(
                "index has an invalid initial checkpoint",
            ));
        }
        for checkpoint in &self.checkpoints {
            if checkpoint.pixel_offset >= total_pixels
                || checkpoint.input_offset < crate::decode::HEADER_SIZE
                || checkpoint.input_offset >= data_end
            {
                return Err(QoiError::InvalidData(
                    "index checkpoint is outside the image",
                ));
            }
        }
        if self.checkpoints.windows(2).any(|pair| {
            pair[0].pixel_offset >= pair[1].pixel_offset
                || pair[0].input_offset >= pair[1].input_offset
        }) {
            return Err(QoiError::InvalidData("index checkpoints are not ordered"));
        }
        Ok(header)
    }
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn fingerprint(bytes: &[u8]) -> u64 {
    // FNV-1a catches stale or mismatched sidecar indexes without a new dependency.
    fingerprint_update(0xcbf29ce484222325, bytes)
}

fn fingerprint_update(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    hash
}
