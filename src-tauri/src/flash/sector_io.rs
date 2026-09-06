use std::io::{self, Read, Seek, SeekFrom, Write};

/// Adapta operações pequenas de MBR/FAT a setores completos do dispositivo.
/// Nenhuma escrita sub-setor chega ao handle bruto do Windows.
pub struct SectorIo<T> {
    inner: T,
    sector: usize,
    length: u64,
    position: u64,
}

impl<T: Read + Write + Seek> SectorIo<T> {
    pub fn finish(mut self) -> io::Result<T> {
        self.inner.flush()?;
        Ok(self.inner)
    }

    pub fn new(inner: T, sector: u32, length: u64) -> io::Result<Self> {
        if !(512..=4096).contains(&sector) || !sector.is_power_of_two()
            || !length.is_multiple_of(u64::from(sector)) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "geometria de setores inválida"));
        }
        Ok(Self { inner, sector: sector as usize, length, position: 0 })
    }
}

impl<T: Read + Write + Seek> Read for SectorIo<T> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let wanted = output.len().min((self.length - self.position) as usize);
        if wanted == 0 { return Ok(0); }
        let offset = self.position as usize % self.sector;
        self.inner.seek(SeekFrom::Start(self.position - offset as u64))?;
        let count = if offset == 0 && wanted >= self.sector {
            let count = wanted / self.sector * self.sector;
            self.inner.read_exact(&mut output[..count])?;
            count
        } else {
            let mut sector = vec![0; self.sector];
            self.inner.read_exact(&mut sector)?;
            let count = wanted.min(self.sector - offset);
            output[..count].copy_from_slice(&sector[offset..offset + count]);
            count
        };
        self.position += count as u64;
        Ok(count)
    }
}

impl<T: Read + Write + Seek> Write for SectorIo<T> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let wanted = input.len().min((self.length - self.position) as usize);
        if wanted == 0 { return Ok(0); }
        let offset = self.position as usize % self.sector;
        let start = self.position - offset as u64;
        self.inner.seek(SeekFrom::Start(start))?;
        let count = if offset == 0 && wanted >= self.sector {
            let count = wanted / self.sector * self.sector;
            self.inner.write_all(&input[..count])?;
            count
        } else {
            let mut sector = vec![0; self.sector];
            self.inner.read_exact(&mut sector)?;
            let count = wanted.min(self.sector - offset);
            sector[offset..offset + count].copy_from_slice(&input[..count]);
            self.inner.seek(SeekFrom::Start(start))?;
            self.inner.write_all(&sector)?;
            count
        };
        self.position += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> { self.inner.flush() }
}

impl<T: Read + Write + Seek> Seek for SectorIo<T> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(value) => i128::from(self.position) + i128::from(value),
            SeekFrom::End(value) => i128::from(self.length) + i128::from(value),
        };
        if position < 0 || position > i128::from(self.length) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "acesso fora do dispositivo"));
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    struct StrictDisk { data: Cursor<Vec<u8>>, sector: usize }
    impl Read for StrictDisk {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            assert_eq!(self.data.position() % self.sector as u64, 0);
            assert_eq!(bytes.len() % self.sector, 0);
            self.data.read(bytes)
        }
    }
    impl Write for StrictDisk {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            assert_eq!(self.data.position() % self.sector as u64, 0);
            assert_eq!(bytes.len() % self.sector, 0);
            self.data.write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }
    impl Seek for StrictDisk {
        fn seek(&mut self, from: SeekFrom) -> io::Result<u64> { self.data.seek(from) }
    }
    #[test]
    fn supports_small_and_cross_sector_writes_without_touching_neighbors() {
        for sector in [512, 4096] {
            let length = sector * 4;
            let disk = StrictDisk { data: Cursor::new(vec![0x55; length]), sector };
            let mut io = SectorIo::new(disk, sector as u32, length as u64).unwrap();
            io.seek(SeekFrom::Start(sector as u64 - 3)).unwrap();
            io.write_all(&vec![0xaa; sector + 7]).unwrap();
            io.seek(SeekFrom::Start(0)).unwrap();
            let mut actual = vec![0; length];
            io.read_exact(&mut actual).unwrap();
            assert!(actual[..sector - 3].iter().all(|b| *b == 0x55));
            assert!(actual[sector - 3..2 * sector + 4].iter().all(|b| *b == 0xaa));
            assert!(actual[2 * sector + 4..].iter().all(|b| *b == 0x55));
            assert!(io.seek(SeekFrom::End(1)).is_err());
            assert!(io.write_all(&[1]).is_err());
        }
    }
}
