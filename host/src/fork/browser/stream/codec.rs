//! Formato interno limitado para quadros; não habilita transporte ou autorização.
use super::{Frame, Size};

const MAGIC: &[u8; 4] = b"MXJ1";
const HEADER: usize = 44;
pub const MAX_JPEG: usize = 512 * 1024;
const MAX_ID: usize = 256;
/// Maior quadro codificado completo (cabeçalho, identificadores e JPEG).
pub const MAX_WIRE: usize = HEADER + 2 * MAX_ID + MAX_JPEG;

#[derive(Debug, PartialEq, Eq)]
pub enum CodecError {
    Invalid,
    TooLarge,
}

fn valid_jpeg(jpeg: &[u8]) -> bool {
    jpeg.len() >= 4 && jpeg.starts_with(&[0xff, 0xd8]) && jpeg.ends_with(&[0xff, 0xd9])
}

pub fn encode(frame: &Frame) -> Result<Vec<u8>, CodecError> {
    let session = frame.session.as_bytes();
    let target = frame.target.as_bytes();
    if frame.jpeg.len() > MAX_JPEG || session.len() > MAX_ID || target.len() > MAX_ID {
        return Err(CodecError::TooLarge);
    }
    if session.is_empty()
        || target.is_empty()
        || !valid_jpeg(&frame.jpeg)
        || [frame.image.width, frame.image.height, frame.viewport.width, frame.viewport.height].contains(&0)
    {
        return Err(CodecError::Invalid);
    }
    let mut out = Vec::with_capacity(HEADER + session.len() + target.len() + frame.jpeg.len());
    out.extend_from_slice(MAGIC);
    let session_len = u16::try_from(session.len()).map_err(|_| CodecError::TooLarge)?;
    let target_len = u16::try_from(target.len()).map_err(|_| CodecError::TooLarge)?;
    let jpeg_len = u32::try_from(frame.jpeg.len()).map_err(|_| CodecError::TooLarge)?;
    out.extend_from_slice(&session_len.to_be_bytes());
    out.extend_from_slice(&target_len.to_be_bytes());
    out.extend_from_slice(&jpeg_len.to_be_bytes());
    out.extend_from_slice(&frame.sequence.to_be_bytes());
    out.extend_from_slice(&frame.target_generation.to_be_bytes());
    for value in [frame.image.width, frame.image.height, frame.viewport.width, frame.viewport.height] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(session);
    out.extend_from_slice(target);
    out.extend_from_slice(&frame.jpeg);
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<Frame, CodecError> {
    if bytes.len() > MAX_WIRE {
        return Err(CodecError::TooLarge);
    }
    if bytes.len() < HEADER || &bytes[..4] != MAGIC {
        return Err(CodecError::Invalid);
    }
    let u16_at = |i| u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
    let u32_at = |i| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let u64_at = |i| u64::from_be_bytes(bytes[i..i + 8].try_into().expect("header checked"));
    let (session_len, target_len, jpeg_len) = (u16_at(4), u16_at(6), u32_at(8) as usize);
    if session_len > MAX_ID || target_len > MAX_ID || jpeg_len > MAX_JPEG {
        return Err(CodecError::TooLarge);
    }
    if session_len == 0 || target_len == 0 || bytes.len() != HEADER + session_len + target_len + jpeg_len {
        return Err(CodecError::Invalid);
    }
    let session_end = HEADER + session_len;
    let target_end = session_end + target_len;
    let session = std::str::from_utf8(&bytes[HEADER..session_end]).map_err(|_| CodecError::Invalid)?;
    let target = std::str::from_utf8(&bytes[session_end..target_end]).map_err(|_| CodecError::Invalid)?;
    let jpeg = &bytes[target_end..];
    let image = Size { width: u32_at(28), height: u32_at(32) };
    let viewport = Size { width: u32_at(36), height: u32_at(40) };
    if !valid_jpeg(jpeg) || [image.width, image.height, viewport.width, viewport.height].contains(&0) {
        return Err(CodecError::Invalid);
    }
    Ok(Frame {
        session: session.into(),
        target: target.into(),
        sequence: u64_at(12),
        target_generation: u64_at(20),
        image,
        viewport,
        jpeg: jpeg.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame {
            session: "sessão".into(),
            target: "aba".into(),
            sequence: 7,
            target_generation: 3,
            image: Size { width: 960, height: 540 },
            viewport: Size { width: 1280, height: 720 },
            jpeg: vec![0xff, 0xd8, 1, 0xff, 0xd9],
        }
    }

    #[test]
    fn round_trip_and_limits() {
        let mut f = frame();
        assert_eq!(decode(&encode(&f).unwrap()), Ok(f.clone()));
        f.jpeg = vec![0; MAX_JPEG];
        f.jpeg[..2].copy_from_slice(&[0xff, 0xd8]);
        f.jpeg[MAX_JPEG - 2..].copy_from_slice(&[0xff, 0xd9]);
        assert_eq!(decode(&encode(&f).unwrap()), Ok(f.clone()));
        f.jpeg.push(0);
        assert_eq!(encode(&f), Err(CodecError::TooLarge));
        assert_eq!(decode(&vec![0; HEADER + 2 * MAX_ID + MAX_JPEG + 1]), Err(CodecError::TooLarge));
    }

    #[test]
    fn rejects_corruption_and_truncation() {
        let wire = encode(&frame()).unwrap();
        for end in 0..wire.len() {
            assert!(decode(&wire[..end]).is_err());
        }
        let mut bad = wire.clone();
        bad[0] ^= 1;
        assert_eq!(decode(&bad), Err(CodecError::Invalid));
        let mut bad = wire.clone();
        bad[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(decode(&bad), Err(CodecError::TooLarge));
        let mut bad = wire.clone();
        *bad.last_mut().unwrap() = 0;
        assert_eq!(decode(&bad), Err(CodecError::Invalid));
        let mut bad = wire;
        bad[HEADER] = 0xff;
        assert_eq!(decode(&bad), Err(CodecError::Invalid));
    }
}
