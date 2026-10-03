use std::io::{self, BufRead, ErrorKind};

pub(crate) fn read_limited_line<R: BufRead>(
    reader: &mut R,
    maximum_bytes: usize,
) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let mut exceeded_limit = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() {
                return if exceeded_limit {
                    Err(io::Error::new(
                        ErrorKind::InvalidData,
                        format!("request frame exceeds {maximum_bytes} bytes"),
                    ))
                } else {
                    Ok(None)
                };
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !exceeded_limit && bytes.len().saturating_add(consumed) > maximum_bytes {
            exceeded_limit = true;
            bytes.clear();
        }
        if !exceeded_limit {
            bytes.extend_from_slice(&available[..consumed]);
        }
        reader.consume(consumed);
        if newline.is_some() {
            if exceeded_limit {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!("request frame exceeds {maximum_bytes} bytes"),
                ));
            }
            break;
        }
    }
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn oversized_and_invalid_utf8_frames_do_not_consume_the_next_frame() {
        let mut reader = Cursor::new(b"too-long\n{}\n");
        assert!(read_limited_line(&mut reader, 4).is_err());
        assert_eq!(
            read_limited_line(&mut reader, 4).unwrap(),
            Some("{}".into())
        );
        let mut reader = Cursor::new(b"\xff\n{}\n");
        assert!(read_limited_line(&mut reader, 4).is_err());
        assert_eq!(
            read_limited_line(&mut reader, 4).unwrap(),
            Some("{}".into())
        );
    }
}
