//! Annex-B bitstream parsing and NAL unit classification for HEVC and H.264.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HevcNalType {
    Vps,
    Sps,
    Pps,
    Idr,
    Cra,
    Slice,
    Other(u8),
}

/// Parse the HEVC NAL unit type from a NAL buffer (with or without start code).
pub fn parse_hevc_nal_type(nal: &[u8]) -> Option<HevcNalType> {
    let payload = strip_start_code(nal);
    if payload.is_empty() {
        return None;
    }
    // HEVC NAL header: forbidden_zero_bit (1 bit) + nal_unit_type (6 bits) + nuh_layer_id (6 bits) + nuh_temporal_id_plus1 (3 bits)
    let nal_unit_type = (payload[0] >> 1) & 0x3F;
    Some(match nal_unit_type {
        32 => HevcNalType::Vps,
        33 => HevcNalType::Sps,
        34 => HevcNalType::Pps,
        19 | 20 => HevcNalType::Idr,
        21 => HevcNalType::Cra,
        0..=9 => HevcNalType::Slice,
        other => HevcNalType::Other(other),
    })
}

pub fn is_hevc_parameter_set(nal: &[u8]) -> bool {
    matches!(
        parse_hevc_nal_type(nal),
        Some(HevcNalType::Vps | HevcNalType::Sps | HevcNalType::Pps)
    )
}

pub fn is_hevc_keyframe(nal: &[u8]) -> bool {
    matches!(
        parse_hevc_nal_type(nal),
        Some(HevcNalType::Idr | HevcNalType::Cra)
    )
}

/// Strip Annex-B start code (3-byte 0x000001 or 4-byte 0x00000001) from beginning of slice.
pub fn strip_start_code(nal: &[u8]) -> &[u8] {
    if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        nal
    }
}

/// Split an Annex-B formatted byte stream into individual NAL units, retaining start codes.
pub fn split_annex_b(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut indices = Vec::new();

    let mut i = 0;
    while i + 2 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                indices.push(i);
                i += 3;
                continue;
            } else if i + 3 < data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                indices.push(i);
                i += 4;
                continue;
            }
        }
        i += 1;
    }

    for (k, &start) in indices.iter().enumerate() {
        let end = if k + 1 < indices.len() {
            indices[k + 1]
        } else {
            data.len()
        };
        if start < end {
            nals.push(&data[start..end]);
        }
    }

    nals
}

/// Group complete HEVC access units, keeping multi-slice pictures together.
/// AUD and prefix metadata belong to the following picture; suffix SEI stays with its picture.
pub fn hevc_access_units(data: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut current = Vec::new();
    let mut has_slice = false;
    for nal in split_annex_b(data) {
        let payload = strip_start_code(nal);
        if payload.len() < 2 {
            continue;
        }
        let kind = (payload[0] >> 1) & 63;
        let is_slice = kind <= 31;
        let first_slice = is_slice && payload.get(2).is_some_and(|b| b & 0x80 != 0);
        let starts_next = matches!(kind, 32..=35 | 39) || first_slice;
        if has_slice && starts_next {
            frames.push(std::mem::take(&mut current));
            has_slice = false;
        }
        current.extend_from_slice(nal);
        has_slice |= is_slice;
    }
    if has_slice {
        frames.push(current);
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_multiple_slices_and_prefix_suffix_metadata() {
        let parts: [&[u8]; 7] = [
            &[0, 0, 1, 64, 1, 64],  // VPS
            &[0, 0, 1, 38, 1, 128], // first IDR slice
            &[0, 0, 1, 38, 1, 64],  // continuation slice
            &[0, 0, 1, 80, 1, 64],  // suffix SEI
            &[0, 0, 1, 78, 1, 64],  // prefix SEI for next frame
            &[0, 0, 1, 2, 1, 128],
            &[0, 0, 1, 2, 1, 64],
        ];
        let frames = hevc_access_units(&parts.concat());
        assert_eq!(frames, vec![parts[..4].concat(), parts[4..].concat()]);
    }

    #[test]
    fn test_split_and_classify_hevc_nals() {
        // Construct Annex-B stream: VPS (type 32 -> byte 0x40), SPS (type 33 -> byte 0x42), PPS (type 34 -> byte 0x44), IDR (type 19 -> byte 0x26)
        let mut stream = Vec::new();
        stream.extend_from_slice(&[0, 0, 0, 1, 0x40, 0x01, 0x0c]); // VPS
        stream.extend_from_slice(&[0, 0, 0, 1, 0x42, 0x01, 0x01]); // SPS
        stream.extend_from_slice(&[0, 0, 1, 0x44, 0x01]); // PPS (3-byte start code)
        stream.extend_from_slice(&[0, 0, 0, 1, 0x26, 0x01, 0xaf]); // IDR

        let nals = split_annex_b(&stream);
        assert_eq!(nals.len(), 4);

        assert_eq!(parse_hevc_nal_type(nals[0]), Some(HevcNalType::Vps));
        assert!(is_hevc_parameter_set(nals[0]));

        assert_eq!(parse_hevc_nal_type(nals[1]), Some(HevcNalType::Sps));
        assert!(is_hevc_parameter_set(nals[1]));

        assert_eq!(parse_hevc_nal_type(nals[2]), Some(HevcNalType::Pps));
        assert!(is_hevc_parameter_set(nals[2]));

        assert_eq!(parse_hevc_nal_type(nals[3]), Some(HevcNalType::Idr));
        assert!(is_hevc_keyframe(nals[3]));
        assert!(!is_hevc_parameter_set(nals[3]));
    }
}
