//! A zip writer with just what a workbook needs: deflated entries, the last one streamed.

use std::io::{self, Write};

use flate2::Compression;
use flate2::write::DeflateEncoder;

/// Just enough of a zip writer for a workbook: deflated entries, the last
/// one streamed with a data descriptor. No zip64, so 4 GiB per entry.
pub(super) struct Zip<W: Write> {
    out: W,
    pub(super) written: u64,
    entries: Vec<ZipEntry>,
}

struct ZipEntry {
    name: String,
    flags: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

/// 1980-01-01 00:00, the earliest DOS date.
const DOS_DATE: u16 = 0x21;

fn too_large() -> io::Error {
    io::Error::other("The sheet is too large for an Excel file. Export as CSV instead.")
}

impl<W: Write> Write for Zip<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.out.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

impl<W: Write> Zip<W> {
    pub(super) fn new(out: W) -> Self {
        Self {
            out,
            written: 0,
            entries: Vec::new(),
        }
    }

    fn header(&mut self, entry: &ZipEntry) -> io::Result<()> {
        let mut h = Vec::with_capacity(30 + entry.name.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes()); // version needed
        h.extend_from_slice(&entry.flags.to_le_bytes());
        h.extend_from_slice(&8u16.to_le_bytes()); // deflate
        h.extend_from_slice(&0u16.to_le_bytes()); // time
        h.extend_from_slice(&DOS_DATE.to_le_bytes());
        h.extend_from_slice(&entry.crc.to_le_bytes());
        h.extend_from_slice(&entry.compressed.to_le_bytes());
        h.extend_from_slice(&entry.size.to_le_bytes());
        h.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes()); // extra
        h.extend_from_slice(entry.name.as_bytes());
        self.write_all(&h)
    }

    fn offset(&self) -> io::Result<u32> {
        u32::try_from(self.written).map_err(|_| too_large())
    }

    /// A whole entry at once.
    pub(super) fn file(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data)?;
        let deflated = enc.finish()?;
        let entry = ZipEntry {
            name: name.into(),
            flags: 0,
            crc: crc32fast::hash(data),
            compressed: deflated.len() as u32,
            size: data.len() as u32,
            offset: self.offset()?,
        };
        self.header(&entry)?;
        self.write_all(&deflated)?;
        self.entries.push(entry);
        Ok(())
    }

    /// An entry whose sizes and CRC follow its data (`end_streamed`).
    pub(super) fn start_streamed(&mut self, name: &str) -> io::Result<()> {
        let entry = ZipEntry {
            name: name.into(),
            flags: 0x08,
            crc: 0,
            compressed: 0,
            size: 0,
            offset: self.offset()?,
        };
        self.header(&entry)?;
        self.entries.push(entry);
        Ok(())
    }

    pub(super) fn end_streamed(&mut self, crc: u32, compressed: u64, size: u64) -> io::Result<()> {
        let compressed = u32::try_from(compressed).map_err(|_| too_large())?;
        let size = u32::try_from(size).map_err(|_| too_large())?;
        let mut d = Vec::with_capacity(16);
        d.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
        d.extend_from_slice(&crc.to_le_bytes());
        d.extend_from_slice(&compressed.to_le_bytes());
        d.extend_from_slice(&size.to_le_bytes());
        self.write_all(&d)?;
        let entry = self.entries.last_mut().expect("started");
        entry.crc = crc;
        entry.compressed = compressed;
        entry.size = size;
        Ok(())
    }

    /// Writes the central directory and hands back the output.
    pub(super) fn finish(mut self) -> io::Result<W> {
        let start = self.offset()?;
        let entries = std::mem::take(&mut self.entries);
        for e in &entries {
            let mut c = Vec::with_capacity(46 + e.name.len());
            c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            c.extend_from_slice(&20u16.to_le_bytes()); // made by
            c.extend_from_slice(&20u16.to_le_bytes()); // needed
            c.extend_from_slice(&e.flags.to_le_bytes());
            c.extend_from_slice(&8u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&DOS_DATE.to_le_bytes());
            c.extend_from_slice(&e.crc.to_le_bytes());
            c.extend_from_slice(&e.compressed.to_le_bytes());
            c.extend_from_slice(&e.size.to_le_bytes());
            c.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
            c.extend_from_slice(&e.offset.to_le_bytes());
            c.extend_from_slice(e.name.as_bytes());
            self.write_all(&c)?;
        }
        let size = self.offset()? - start;
        let mut end = Vec::with_capacity(22);
        end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        end.extend_from_slice(&[0; 4]); // disk numbers
        end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&size.to_le_bytes());
        end.extend_from_slice(&start.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        self.write_all(&end)?;
        Ok(self.out)
    }
}
