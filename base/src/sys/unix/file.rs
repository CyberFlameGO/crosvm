// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![deny(missing_docs)]

use std::ops::Range;

use crate::error;
use crate::AsRawDescriptor;
use crate::Error;
use crate::Result;

enum LseekOption {
    Data,
    Hole,
}

fn lseek(fd: &dyn AsRawDescriptor, offset: u64, option: LseekOption) -> Result<u64> {
    let whence = match option {
        LseekOption::Data => libc::SEEK_DATA,
        LseekOption::Hole => libc::SEEK_HOLE,
    };
    // safe because this doesn't modify any memory.
    let ret = unsafe { libc::lseek64(fd.as_raw_descriptor(), offset as i64, whence) };
    if ret < 0 {
        return Err(Error::last());
    }
    Ok(ret as u64)
}

/// Iterator returning the offset range of data in the file.
///
/// This uses `lseek(2)` internally, and thus it changes the file offset.
pub struct FileDataIterator<'a> {
    fd: &'a dyn AsRawDescriptor,
    offset: u64,
}

impl<'a> FileDataIterator<'a> {
    /// Creates the [FileDataIterator]
    ///
    /// # Arguments
    ///
    /// * `fd` - the [trait@AsRawDescriptor] of the file
    /// * `offset` - the offset to start traversing from.
    pub fn new(fd: &'a dyn AsRawDescriptor, offset: u64) -> Self {
        Self { fd, offset }
    }

    fn find_next_data(&self) -> Result<Option<Range<u64>>> {
        let offset_data = match lseek(self.fd, self.offset, LseekOption::Data) {
            Ok(offset) => offset,
            Err(e) => {
                return match e.errno() {
                    libc::ENXIO => Ok(None),
                    _ => Err(e),
                }
            }
        };
        let offset_hole = lseek(self.fd, offset_data, LseekOption::Hole)?;

        Ok(Some(offset_data..offset_hole))
    }
}

impl<'a> Iterator for FileDataIterator<'a> {
    type Item = Range<u64>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.find_next_data() {
            Ok(data_range) => {
                if let Some(ref data_range) = data_range {
                    self.offset = data_range.end;
                }
                data_range
            }
            Err(e) => {
                error!("failed to get data range: {:?}", e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::FileExt;

    use crate::pagesize;

    use super::*;

    #[test]
    fn file_data_iterator() {
        let file = tempfile::tempfile().unwrap();

        file.write_at(&[1_u8], 10).unwrap();
        file.write_at(&[1_u8], 2 * pagesize() as u64).unwrap();
        file.write_at(&[1_u8], (4 * pagesize() - 1) as u64).unwrap();

        let iter = FileDataIterator::new(&file, 0);

        let result: Vec<Range<u64>> = iter.collect();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], 0..(pagesize() as u64));
        assert_eq!(result[1], (2 * pagesize() as u64)..(4 * pagesize() as u64));
    }
}
