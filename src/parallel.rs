use crate::decode::{DecoderState, Header, QoiError, decode, decode_range};
use crate::index::DecodeIndex;
use rayon::scope;
use std::sync::Mutex;

/// A validated QOI stream and index that can be decoded repeatedly.
///
/// The immutable borrow keeps the indexed bytes unchanged while the binding
/// is alive, so the file fingerprint only needs to be checked once.
pub struct IndexedDecoder<'a> {
    bytes: &'a [u8],
    index: IndexStorage<'a>,
    header: Header,
}

enum IndexStorage<'a> {
    Borrowed(&'a DecodeIndex),
    Owned(DecodeIndex),
}

impl IndexStorage<'_> {
    fn get(&self) -> &DecodeIndex {
        match self {
            Self::Borrowed(index) => index,
            Self::Owned(index) => index,
        }
    }
}

impl<'a> IndexedDecoder<'a> {
    pub fn new(bytes: &'a [u8], index: &'a DecodeIndex) -> Result<Self, QoiError> {
        let header = index.validate(bytes)?;
        if index.checkpoints.is_empty() || index.checkpoints[0].pixel_offset != 0 {
            return Err(QoiError::InvalidData("index has no initial checkpoint"));
        }
        Ok(Self {
            bytes,
            index: IndexStorage::Borrowed(index),
            header,
        })
    }

    /// Builds and binds an index in one pass over the QOI data.
    pub fn build(bytes: &'a [u8], workers: usize) -> Result<Self, QoiError> {
        let header = crate::decode::read_header(bytes)?;
        let index = DecodeIndex::build(bytes, workers)?;
        Ok(Self {
            bytes,
            index: IndexStorage::Owned(index),
            header,
        })
    }

    pub fn decode(&self) -> Result<(Header, Vec<u8>), QoiError> {
        let index = self.index.get();
        if index.checkpoints.len() == 1 {
            return decode(self.bytes);
        }

        let header = self.header;
        let channels = usize::from(header.channels);
        let total_pixels = header.pixel_count()?;
        let mut output = vec![0; header.byte_count()?];
        let mut remaining = output.as_mut_slice();
        let first_error = Mutex::new(None);

        scope(|scope| {
            for (position, initial_state) in index.checkpoints.iter().enumerate() {
                let end_pixel = index
                    .checkpoints
                    .get(position + 1)
                    .map_or(total_pixels, |checkpoint| checkpoint.pixel_offset);
                let pixel_count = end_pixel.saturating_sub(initial_state.pixel_offset);
                let segment_bytes = pixel_count * channels;
                let (segment, rest) = remaining.split_at_mut(segment_bytes);
                remaining = rest;
                let state: DecoderState = initial_state.clone();
                let first_error = &first_error;
                scope.spawn(move |_| {
                    if let Err(error) = decode_range(self.bytes, header, state, end_pixel, segment)
                    {
                        let mut stored_error = first_error.lock().expect("error mutex poisoned");
                        if stored_error.is_none() {
                            *stored_error = Some(error);
                        }
                    }
                });
            }
        });

        if let Some(error) = first_error.into_inner().expect("error mutex poisoned") {
            return Err(error);
        }
        Ok((header, output))
    }
}

pub fn decode_indexed(
    bytes: &[u8],
    index: &DecodeIndex,
) -> Result<(crate::decode::Header, Vec<u8>), QoiError> {
    IndexedDecoder::new(bytes, index)?.decode()
}
