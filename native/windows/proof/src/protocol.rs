//! Independent reader for the frozen version-four diagnostic wire format.
//!
//! This module deliberately owns its schema. It imports no service, engine, or
//! production diagnostic implementation.

/// Diagnostic mapping protocol version.
pub const PROTOCOL: u32 = 4;
/// Fixed mapping extent in bytes.
pub const PAGE_BYTES: usize = 4096;
/// Fixed number of DWORDs in a payload.
pub const PAYLOAD_WORDS: usize = 512;
/// First payload word of the command-observation window.
pub const OBSERVATION_OFFSET: usize = 177;
/// Number of retained lifecycle events.
pub const LIFECYCLE_CAPACITY: usize = 16;
/// Number of retained command observations.
pub const OBSERVATION_CAPACITY: usize = 32;
/// Exactly 512 little-endian DWORD values from a version-four publication.
pub type Payload = [u32; PAYLOAD_WORDS];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Process creation time, native architecture, and admitted diagnostic candidate identity.
pub struct Identity {
    /// Retained process handle or process identifier, as specified by the field type.
    pub process: u32,
    /// Captured native thread identifier.
    pub thread: u32,
    /// Process creation FILETIME as a 64-bit value.
    pub started: u64,
    /// Diagnostic candidate kind, with one proof editor, two packaged Notepad, and three generic host.
    pub kind: u32,
    /// Package major, minor, build, and revision components.
    pub version: [u32; 4],
    /// PE machine value captured independently from the retained process.
    pub architecture: u32,
}

/// Format the frozen process-creation/thread mapping name.
pub fn mapping_name(identity: &Identity) -> String {
    format!(
        "Local\\KeyrillProbe-v{PROTOCOL}-{}-{:016x}-{}",
        identity.process, identity.started, identity.thread
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Validated protocol-four diagnostic sections in their frozen wire order.
pub struct ProbeSnapshot {
    /// Nonzero publisher-instance claim.
    pub instance: u32,
    /// Diagnostic candidate kind, with one proof editor, two packaged Notepad, and three generic host.
    pub kind: u32,
    /// Package major, minor, build, and revision components.
    pub version: [u32; 4],
    /// PE machine value captured independently from the retained process.
    pub architecture: u32,
    /// Eight SHA-256 DWORDs in canonical displayed order.
    pub build: [u32; 8],
    /// Ten host-admission metadata DWORDs.
    pub metadata: [u32; 10],
    /// 55 service diagnostic DWORDs.
    pub service: [u32; 55],
    /// Lifecycle header followed by sixteen five-DWORD event rows.
    pub lifecycle: [u32; 86],
    /// Runtime header and two vocabulary-source status/error pairs.
    pub runtime: [u32; 12],
    /// Observation header followed by thirty-two ten-DWORD rows.
    pub observations: [u32; 323],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// The wire section that failed independent schema validation.
pub enum DecodeError {
    /// The publisher or candidate identity is invalid.
    Identity,
    /// The host-admission metadata is invalid.
    Metadata,
    /// The service diagnostic section is invalid.
    Service,
    /// The lifecycle history section is invalid.
    Lifecycle,
    /// The runtime status section is invalid.
    Runtime,
    /// The command-observation section is invalid.
    Observation,
    /// A reserved payload word is nonzero.
    Reserved,
}

fn valid_refusal(value: &[u32]) -> bool {
    value[0] <= 1
        && value[1] < 26
        && value[2] < 15
        && if value[0] != 0 {
            value[2] != 0
                && if value[2] == 1 {
                    value[3] != 0
                } else {
                    value[3] == 0
                }
        } else {
            value[1..] == [0, 0, 0]
        }
}

pub(crate) fn valid_service(value: &[u32; 55]) -> bool {
    if [1, 2, 4, 5, 7].iter().any(|&i| value[i] > 1)
        || value[3] > 2
        || (value[5] == 0 && value[6] != 0)
        || (value[7] == 0 && value[8] != 0)
        || !valid_refusal(&value[47..51])
        || !valid_refusal(&value[51..55])
        || value[47] != value[51]
    {
        return false;
    }
    let maximum = value[0] == u32::MAX || value[9..47].contains(&u32::MAX);
    let refusal = value[21..47].iter().any(|&word| word != 0);
    (!maximum || value[4] != 0)
        && refusal == (value[47] != 0)
        && (!refusal
            || (value[21 + value[48] as usize] != 0 && value[21 + value[52] as usize] != 0))
}

pub(crate) fn valid_lifecycle(value: &[u32; 86]) -> bool {
    let [sequence, allocated, saturated, count, owned, serial] = value[..6].try_into().unwrap();
    if saturated > 1
        || owned > 1
        || count != sequence.min(16)
        || (owned == 0 && serial != 0)
        || serial > allocated
        || (owned != 0 && serial == 0 && allocated != u32::MAX)
        || ((sequence == u32::MAX || allocated == u32::MAX) && saturated == 0)
    {
        return false;
    }
    for (index, event) in value[6..].as_chunks::<5>().0.iter().enumerate() {
        if index >= count as usize {
            if *event != [0; 5] {
                return false;
            }
            continue;
        }
        let [number, kind, subject, cause, unresolved] = *event;
        if number != sequence - count + 1 + index as u32
            || !(1..10).contains(&kind)
            || cause >= 9
            || unresolved >= 5
            || subject > allocated
            || (kind == 8) != (cause != 0)
            || (kind == 7) != (unresolved != 0)
            || (kind == 6 && subject != 0)
            || ([1, 2, 3, 4, 5, 9].contains(&kind) && subject == 0 && allocated != u32::MAX)
        {
            return false;
        }
    }
    true
}

fn knowledge(word: u32) -> bool {
    word == 0 || word & !0x3f == 0x100
}
fn exact(word: u32) -> bool {
    word == 0x103
}
fn same_command(a: &[u32], b: &[u32]) -> bool {
    a[5..8] == b[5..8]
}

fn valid_observations(value: &[u32; 323], lifecycle: &[u32; 86]) -> bool {
    let sequence = value[0];
    let count = value[2];
    if count != sequence.min(32) || value[1] != u32::from(sequence == u32::MAX) {
        return false;
    }
    let oldest = sequence - count;
    let mut cursor = 0;
    let rows = value[3..].as_chunks::<10>().0;
    for (index, row) in rows.iter().enumerate() {
        if index >= count as usize {
            if *row != [0; 10] {
                return false;
            }
            continue;
        }
        if row[0] != oldest + index as u32 + 1
            || row[1] < cursor
            || row[1] > lifecycle[0]
            || row[2] > 1
            || row[3] > lifecycle[1]
            || (row[2] == 0 && row[3] != 0)
            || (row[2] != 0 && row[3] == 0 && lifecycle[1] != u32::MAX)
        {
            return false;
        }
        cursor = row[1];
        match row[4] {
            7 => {
                if row[5] > 1
                    || row[6] > 2
                    || row[7] != 0
                    || !(1..=6).contains(&row[8])
                    || !(1..=2).contains(&row[9])
                    || (row[6] != 0 && row[8] != 3)
                    || (row[5] != 0 && (row[8] != 3 || row[6] != 1))
                {
                    return false;
                }
            }
            8 => {
                if row[5..8] != [0; 3] || row[9] != 0 || !(1..=3).contains(&row[8]) {
                    return false;
                }
            }
            9 => {
                if !(1..=2).contains(&row[5])
                    || !(1..=2).contains(&row[6])
                    || row[7] == 0
                    || row[7] >= row[0]
                    || row[8] > 1
                    || !(1..=4).contains(&row[9])
                {
                    return false;
                }
                let mut dispatch = None;
                for earlier in &rows[..index] {
                    if earlier[7] != row[7] {
                        continue;
                    }
                    if earlier[4] == 9 {
                        return false;
                    }
                    if earlier[4] <= 6 {
                        if !same_command(earlier, row) {
                            return false;
                        }
                        dispatch = Some(earlier[4]);
                    }
                }
                if dispatch.map_or(row[7] > oldest, |stage| stage != 5) {
                    return false;
                }
            }
            stage @ 1..=6 => {
                if !(1..=2).contains(&row[5])
                    || !(1..=2).contains(&row[6])
                    || row[7] == 0
                    || row[7] > row[0]
                    || row[9] != 0
                    || !knowledge(row[8])
                    || ((3..=5).contains(&stage) && row[8] == 0)
                    || (stage == 1 && row[7] != row[0])
                    || (stage != 1 && row[7] == row[0])
                    || (stage == 1 && row[5] == 2 && row[8] != 0)
                    || (row[5] == 1 && !exact(row[8]))
                    || ([2, 3].contains(&stage) && row[5] != 2)
                    || (stage == 3 && exact(row[8]))
                    || ([4, 5].contains(&stage) && !exact(row[8]))
                {
                    return false;
                }
                let mut previous = None;
                for earlier in &rows[..index] {
                    if earlier[4] > 6 || earlier[7] != row[7] {
                        continue;
                    }
                    if !same_command(earlier, row) || (earlier[8] != 0 && earlier[8] != row[8]) {
                        return false;
                    }
                    previous = Some(earlier[4]);
                }
                if previous.map_or(stage != 1 && row[7] > oldest, |before| {
                    stage == 1 || (before != 1 && !(before == 5 && stage == 6))
                }) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Decode and validate every frozen wire section and all reserved DWORDs.
///
/// # Errors
/// Returns the first failed native operation or evidence predicate.
pub fn decode_payload(words: &Payload) -> Result<ProbeSnapshot, DecodeError> {
    let result = ProbeSnapshot {
        architecture: words[500],
        instance: words[0],
        kind: words[1],
        version: std::array::from_fn(|index| words[2 + index]),
        build: std::array::from_fn(|index| words[6 + index]),
        metadata: std::array::from_fn(|index| words[14 + index]),
        service: std::array::from_fn(|index| words[24 + index]),
        lifecycle: std::array::from_fn(|index| words[79 + index]),
        runtime: std::array::from_fn(|index| words[165 + index]),
        observations: std::array::from_fn(|index| words[177 + index]),
    };
    if result.instance == 0
        || !(1..=3).contains(&result.kind)
        || ![0x014c, 0x8664].contains(&result.architecture)
        || (result.kind != 2 && result.version != [0; 4])
        || (result.kind == 2 && result.architecture != 0x8664)
    {
        return Err(DecodeError::Identity);
    }
    let m = result.metadata;
    if m[0] > 16 || m[2] > 3 || m[3] > 5 || m[6] > 1 || m[7] > m[6] || m[8] > 1 || m[9] > m[8] {
        return Err(DecodeError::Metadata);
    }
    if !valid_service(&result.service) {
        return Err(DecodeError::Service);
    }
    if !valid_lifecycle(&result.lifecycle) {
        return Err(DecodeError::Lifecycle);
    }
    let r = result.runtime;
    if r[1] > 2
        || r[2] > 2
        || r[3] > 1
        || r[6] > 9
        || r[7] > 9
        || r[8] > 3
        || r[9] > 9
        || r[10] > 3
        || r[11] > 9
    {
        return Err(DecodeError::Runtime);
    }
    if !valid_observations(&result.observations, &result.lifecycle) {
        return Err(DecodeError::Observation);
    }
    if words[501..].iter().any(|&word| word != 0) {
        return Err(DecodeError::Reserved);
    }
    Ok(result)
}

/// Require source continuity and retained overlapping event histories.
pub fn complete_interval(before: &ProbeSnapshot, after: &ProbeSnapshot) -> bool {
    if before.instance == 0
        || before.instance != after.instance
        || before.build != after.build
        || before.architecture != after.architecture
        || before.kind != after.kind
        || before.version != after.version
        || before.service[0] == 0
        || before.service[0] != after.service[0]
        || before.service[4] != 0
        || after.service[4] != 0
        || before.lifecycle[2] != 0
        || after.lifecycle[2] != 0
        || before.observations[1] != 0
        || after.observations[1] != 0
        || !valid_service(&before.service)
        || !valid_service(&after.service)
        || !valid_lifecycle(&before.lifecycle)
        || !valid_lifecycle(&after.lifecycle)
        || !valid_observations(&before.observations, &before.lifecycle)
        || !valid_observations(&after.observations, &after.lifecycle)
    {
        return false;
    }
    for (a, b, width, header) in [
        (&before.lifecycle[..], &after.lifecycle[..], 5, 6),
        (&before.observations[..], &after.observations[..], 10, 3),
    ] {
        let count_index = if header == 6 { 3 } else { 2 };
        if b[0] < a[0] || b[0] - a[0] > b[count_index] {
            return false;
        }
        let oldest = b[0] - b[count_index];
        for event in a[header..header + a[count_index] as usize * width].chunks_exact(width) {
            if event[0] > oldest {
                let offset = header + (event[0] - oldest - 1) as usize * width;
                if event != &b[offset..offset + width] {
                    return false;
                }
            }
        }
    }
    if after.lifecycle[1] < before.lifecycle[1] {
        return false;
    }
    let a = &before.observations;
    let b = &after.observations;
    b[0] == a[0] || b[3 + (a[0] - (b[0] - b[2])) as usize * 10 + 1] >= before.lifecycle[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn empty() -> Payload {
        let mut words = [0; 512];
        words[0] = 1;
        words[1] = 3;
        words[500] = 0x8664;
        words[24] = 1;
        words
    }
    #[test]
    fn frozen_protocol4_offsets_and_mapping_name() {
        let mut words = empty();
        words[165] = 123;
        words[169] = 0x89abcdef;
        words[170] = 0x12345678;
        let decoded = decode_payload(&words).unwrap();
        assert_eq!(decoded.runtime[0], 123);
        assert_eq!(
            u64::from(decoded.runtime[4]) | (u64::from(decoded.runtime[5]) << 32),
            0x1234567889abcdef
        );
        assert_eq!(
            mapping_name(&Identity {
                process: 123,
                thread: 456,
                started: 0xabc,
                ..Identity::default()
            }),
            "Local\\KeyrillProbe-v4-123-0000000000000abc-456"
        );
        assert!(complete_interval(&decoded, &decoded));
    }
    #[test]
    fn generic_architectures_and_continuity_are_bound_to_exact_identity() {
        let mut words = empty();
        for machine in [0x014c, 0x8664] {
            words[500] = machine;
            let before = decode_payload(&words).unwrap();
            assert!(complete_interval(&before, &before));
            let mut changed = before.clone();
            changed.architecture ^= 1;
            assert!(!complete_interval(&before, &changed));
            changed = before.clone();
            changed.build[0] ^= 1;
            assert!(!complete_interval(&before, &changed));
            changed = before.clone();
            changed.instance += 1;
            assert!(!complete_interval(&before, &changed));
        }
        words[1] = 2;
        words[2..6].copy_from_slice(&[11, 2510, 6, 0]);
        words[500] = 0x014c;
        assert_eq!(decode_payload(&words), Err(DecodeError::Identity));
        words[500] = 0x8664;
        assert!(decode_payload(&words).is_ok());
        for index in 501..PAYLOAD_WORDS {
            words[index] = 1;
            assert_eq!(decode_payload(&words), Err(DecodeError::Reserved));
            words[index] = 0;
        }
    }
    #[test]
    fn malformed_headers_enums_reserved_and_saturation_are_rejected() {
        for (index, value) in [
            (0, 0),
            (1, 0),
            (1, 4),
            (2, 1),
            (500, 0),
            (500, 0xaa64),
            (14, 17),
            (16, 4),
            (17, 6),
            (20, 2),
            (21, 1),
            (23, 1),
            (25, 2),
            (27, 3),
            (30, 1),
            (79, 1),
            (81, 2),
            (166, 3),
            (168, 2),
            (171, 10),
            (173, 4),
            (178, 1),
            (179, 33),
            (511, 1),
        ] {
            let mut words = empty();
            words[index] = value;
            assert!(decode_payload(&words).is_err(), "word {index}");
        }
    }
    #[test]
    fn independent_lifecycle_and_observation_golden_rows() {
        let mut words = empty();
        words[79..90].copy_from_slice(&[1, 1, 0, 1, 1, 1, 1, 1, 1, 0, 0]);
        words[177..190].copy_from_slice(&[1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0x103, 0]);
        let before = decode_payload(&words).unwrap();
        words[177] = 2;
        words[179] = 2;
        words[190..200].copy_from_slice(&[2, 1, 1, 1, 5, 1, 1, 1, 0x103, 0]);
        let after = decode_payload(&words).unwrap();
        assert!(complete_interval(&before, &after));
        words[198] = 0x101;
        assert_eq!(decode_payload(&words), Err(DecodeError::Observation));
    }
}
