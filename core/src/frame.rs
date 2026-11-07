//! DeLorme's binary message format, as inReach Sync 1.4.47 sends it
//! (`DeLorme.Common.Devices.Usb.DeLormeBinary`).
//!
//! A message is a header — `0xFEDB`, its code, the length of its data plus 4
//! when it has data, and a checksum of those six bytes — followed, when it has
//! data, by the data, a checksum of the data and `0xBCAD`. All values are
//! little-endian. A checksum is `0x10000` minus the sum of the 16-bit words.
//!
//! On USB a message travels in 64-byte HID reports without report IDs. Each
//! output report is `0x20`, the length of its part (up to 62 bytes), then the
//! part; input reports carry the same two-byte prefix.

pub const HEADER: u16 = 0xFEDB;
pub const TRAILER: u16 = 0xBCAD;
pub const LEAD: u8 = 0x20;
pub const REPORT_LEN: usize = 64;
pub const PART_LEN: usize = REPORT_LEN - 2;
/// inReach Sync drops messages that declare more data than this.
pub const MAX_DATA: usize = 4088;

/// The 16-bit checksum of DeLormeBinary.CheckSum: the two's complement of
/// the little-endian word sum, an odd last byte counting as a low byte.
pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    for pair in data.chunks(2) {
        sum = sum.wrapping_add(pair[0] as u32);
        if let Some(&high) = pair.get(1) {
            sum = sum.wrapping_add((high as u32) << 8);
        }
    }
    (0x1_0000u32.wrapping_sub(sum & 0xFFFF) & 0xFFFF) as u16
}

/// The bytes of one message.
pub fn encode(code: u16, data: &[u8]) -> Vec<u8> {
    let length: u16 = if data.is_empty() { 0 } else { (data.len() + 4) as u16 };
    let mut out = Vec::with_capacity(8 + data.len() + 4);
    out.extend_from_slice(&HEADER.to_le_bytes());
    out.extend_from_slice(&code.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    let head = checksum(&out);
    out.extend_from_slice(&head.to_le_bytes());
    if !data.is_empty() {
        out.extend_from_slice(data);
        out.extend_from_slice(&checksum(data).to_le_bytes());
        out.extend_from_slice(&TRAILER.to_le_bytes());
    }
    out
}

/// The output reports of a message, each `REPORT_LEN` bytes after a leading
/// report number 0, which is how hidraw takes a report from a device without
/// numbered reports.
pub fn reports(message: &[u8]) -> Vec<[u8; REPORT_LEN + 1]> {
    let mut out = Vec::new();
    let mut parts = message.chunks(PART_LEN).peekable();
    if parts.peek().is_none() {
        out.push(report(&[]));
    }
    for part in parts {
        out.push(report(part));
    }
    out
}

fn report(part: &[u8]) -> [u8; REPORT_LEN + 1] {
    let mut r = [0u8; REPORT_LEN + 1];
    r[1] = LEAD;
    r[2] = part.len() as u8;
    r[3..3 + part.len()].copy_from_slice(part);
    r
}

/// The part an input report carries; `None` when its length byte does not fit.
pub fn part_of(report: &[u8]) -> Option<&[u8]> {
    let len = *report.get(1)? as usize;
    report.get(2..2 + len)
}

/// A message as it comes from the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub code: u16,
    pub data: Vec<u8>,
}

/// What `Assembler::push` made of a part.
#[derive(Debug, PartialEq, Eq)]
pub enum Pushed {
    /// More parts are needed (or the part was not the start of a message).
    Pending,
    /// A whole message.
    Done(Message),
    /// The message was complete but its data checksum or trailer was wrong.
    Corrupt(u16),
}

/// Puts the parts of input reports together into messages, as
/// DeLormeBinary.Receive does: a part that does not start with a valid
/// header is skipped.
#[derive(Default)]
pub struct Assembler {
    current: Option<(u16, usize, Vec<u8>)>,
}

impl Assembler {
    pub fn push(&mut self, part: &[u8]) -> Pushed {
        if let Some((code, length, mut body)) = self.current.take() {
            body.extend_from_slice(part);
            return self.finish(code, length, body);
        }
        if part.len() < 8
            || u16::from_le_bytes([part[0], part[1]]) != HEADER
            || u16::from_le_bytes([part[6], part[7]]) != checksum(&part[..6])
        {
            return Pushed::Pending;
        }
        let code = u16::from_le_bytes([part[2], part[3]]);
        let length = u16::from_le_bytes([part[4], part[5]]) as usize;
        if length > MAX_DATA {
            return Pushed::Pending;
        }
        self.finish(code, length, part[8..].to_vec())
    }

    fn finish(&mut self, code: u16, length: usize, mut body: Vec<u8>) -> Pushed {
        if body.len() < length {
            self.current = Some((code, length, body));
            return Pushed::Pending;
        }
        body.truncate(length);
        if length == 0 {
            return Pushed::Done(Message { code, data: Vec::new() });
        }
        if length < 4 {
            return Pushed::Corrupt(code);
        }
        let check = u16::from_le_bytes([body[length - 4], body[length - 3]]);
        let trailer = u16::from_le_bytes([body[length - 2], body[length - 1]]);
        body.truncate(length - 4);
        if trailer != TRAILER || check != checksum(&body) {
            return Pushed::Corrupt(code);
        }
        Pushed::Done(Message { code, data: body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_is_the_complement_of_the_word_sum() {
        assert_eq!(checksum(&[]), 0);
        assert_eq!(checksum(&[1, 0]), 0xFFFF);
        assert_eq!(checksum(&[1]), 0xFFFF);
        let head = [0xDB, 0xFE, 0x08, 0xFC, 0, 0];
        let sum = 0xFEDBu32 + 0xFC08 + checksum(&head) as u32;
        assert_eq!(sum & 0xFFFF, 0);
    }

    #[test]
    fn a_message_without_data_is_its_header() {
        let m = encode(0xFC08, &[]);
        assert_eq!(m.len(), 8);
        assert_eq!(&m[..6], &[0xDB, 0xFE, 0x08, 0xFC, 0, 0]);
    }

    #[test]
    fn a_message_with_data_has_its_length_plus_four_and_a_trailer() {
        let m = encode(0xFC02, &[5, 1]);
        assert_eq!(u16::from_le_bytes([m[4], m[5]]), 6);
        assert_eq!(&m[8..10], &[5, 1]);
        assert_eq!(u16::from_le_bytes([m[10], m[11]]), checksum(&[5, 1]));
        assert_eq!(u16::from_le_bytes([m[12], m[13]]), TRAILER);
    }

    #[test]
    fn reports_carry_the_lead_byte_and_the_part_length() {
        let long = encode(0xFC03, &(0..100).collect::<Vec<u8>>()); // 112 bytes
        let r = reports(&long);
        assert_eq!(r.len(), 2);
        assert_eq!(&r[0][..3], &[0, LEAD, 62]);
        assert_eq!(&r[0][3..65], &long[..62]);
        assert_eq!(&r[1][..3], &[0, LEAD, 50]);
        assert_eq!(&r[1][3..53], &long[62..]);
        assert_eq!(&reports(&encode(0xA001, &[]))[0][..3], &[0, LEAD, 8]);
    }

    /// What the device sends: the reports of a message without the report number.
    fn incoming(message: &[u8]) -> Vec<Vec<u8>> {
        reports(message).iter().map(|r| r[1..].to_vec()).collect()
    }

    #[test]
    fn the_assembler_puts_a_long_message_together() {
        let data: Vec<u8> = (0..180).map(|i| i as u8).collect();
        let mut a = Assembler::default();
        let mut got = None;
        for r in incoming(&encode(0x1234, b"junk")).iter().chain(&incoming(&encode(0xA001, &data))) {
            if let Pushed::Done(m) = a.push(part_of(r).unwrap()) {
                got = Some(m);
            }
        }
        assert_eq!(got, Some(Message { code: 0xA001, data }));
    }

    #[test]
    fn the_assembler_reports_a_bad_checksum() {
        let mut m = encode(0xFC08, &[1]);
        m[8] ^= 0xFF;
        let mut a = Assembler::default();
        let r = &incoming(&m)[0];
        assert_eq!(a.push(part_of(r).unwrap()), Pushed::Corrupt(0xFC08));
    }

    #[test]
    fn the_assembler_skips_what_is_not_a_header() {
        let mut a = Assembler::default();
        assert_eq!(a.push(&[1, 2, 3]), Pushed::Pending);
        assert_eq!(a.push(&[0; 10]), Pushed::Pending);
    }
}
