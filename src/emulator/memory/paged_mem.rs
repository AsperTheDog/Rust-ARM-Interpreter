use std::ptr::read_unaligned;
use crate::emulator::Fault;
use crate::emulator::memory::Memory;

const PAGE_SIZE: usize = 4096;
const PAGE_COUNT: usize = 1024 * 1024;
const PAGE_MASK: u32 = 0xFFF;

pub struct PagedMemory {
    table: Vec<Option<Box<[u8; PAGE_SIZE]>>>,

    hot_page_idx: u32,
    hot_page_ptr: *mut u8,
}

impl PagedMemory {
    pub fn new() -> Self {
        Self {
            table: (0..PAGE_COUNT).map(|_| None).collect(),
            hot_page_idx: u32::MAX,
            hot_page_ptr: std::ptr::null_mut(),
        }
    }

    #[inline(always)]
    fn get_page_ptr(&mut self, page_idx: u32) -> Option<*mut u8> {
        if page_idx == self.hot_page_idx {
            return Some(self.hot_page_ptr);
        }

        if let Some(Some(page_box)) = self.table.get_mut(page_idx as usize) {
            self.hot_page_idx = page_idx;
            self.hot_page_ptr = page_box.as_mut_ptr();
            Some(self.hot_page_ptr)
        } else {
            None
        }
    }

    fn create_page(&mut self, page_idx: u32) {
        if self.table[page_idx as usize].is_none() {
            self.table[page_idx as usize] = Some(Box::new([0; PAGE_SIZE]));
        }
    }
}

impl Memory for PagedMemory {
    fn read_u8(&mut self, addr: u32) -> Result<u8, Fault> {
        let page_idx = addr >> 12;
        let offset = (addr & PAGE_MASK) as usize;
        let ptr = self.get_page_ptr(page_idx);

        if ptr.is_none() {
            return Err(Fault::Translation { address: addr, is_write: false })
        }

        Ok(unsafe {
            *ptr.unwrap().add(offset)
        })
    }

    fn write_u8(&mut self, addr: u32, val: u8) -> Result<(), Fault> {
        let page_idx = addr >> 12;
        let offset = (addr & PAGE_MASK) as usize;

        self.create_page(page_idx);

        let ptr = self.get_page_ptr(page_idx);
        unsafe {
            *ptr.unwrap().add(offset) = val;
        }
        Ok(())
    }

    fn load_at(&mut self, mut addr: u32, mut data: &[u8]) -> Result<(), Fault> {
        while !data.is_empty() {
            let page_idx = addr >> 12;
            let offset = (addr & PAGE_MASK) as usize;
            let bytes_left_in_page = PAGE_SIZE - offset;

            let chunk_size = std::cmp::min(data.len(), bytes_left_in_page);

            self.create_page(page_idx);

            let page_ptr = self.get_page_ptr(page_idx);
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    page_ptr.unwrap().add(offset),
                    chunk_size
                );
            }

            addr += chunk_size as u32;
            data = &data[chunk_size..];
        }
        Ok(())
    }

    fn read_u32_le(&mut self, addr: u32) -> Result<u32, Fault> {
        let page_idx = addr >> 12;
        let offset = (addr & PAGE_MASK) as usize;

        if offset <= PAGE_SIZE - 4 {
            let ptr = self.get_page_ptr(page_idx);
            if let Some(ptr) = ptr {
                return unsafe {
                    let word_ptr = ptr.add(offset) as *const u32;
                    Ok(u32::from_le(read_unaligned(word_ptr)))
                };
            }
            return Err(Fault::Translation { address: addr, is_write: false })
        }

        Ok(u32::from_le_bytes([
            self.read_u8(addr)?,
            self.read_u8(addr + 1)?,
            self.read_u8(addr + 2)?,
            self.read_u8(addr + 3)?,
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // read_u8 / write_u8

    #[test]
    fn read_unmapped_page_returns_translation_fault() {
        let mut mem = PagedMemory::new();
        let result = mem.read_u8(0x1000);
        match result {
            Err(Fault::Translation { address, is_write }) => {
                assert_eq!(address, 0x1000);
                assert!(!is_write);
            }
            _ => panic!("expected Translation fault, got {:?}", result),
        }
    }

    #[test]
    fn write_then_read_byte_roundtrip() {
        let mut mem = PagedMemory::new();
        mem.write_u8(0x1000, 0xAB).unwrap();
        assert_eq!(mem.read_u8(0x1000).unwrap(), 0xAB);
    }

    #[test]
    fn write_auto_allocates_page() {
        let mut mem = PagedMemory::new();
        // Writing to an unmapped address should succeed (auto-allocate).
        assert!(mem.write_u8(0xDEAD_0000, 0x42).is_ok());
        assert_eq!(mem.read_u8(0xDEAD_0000).unwrap(), 0x42);
    }

    #[test]
    fn unwritten_bytes_in_allocated_page_read_as_zero() {
        let mut mem = PagedMemory::new();
        // Writing one byte allocates the whole page; the rest must be zero-initialized.
        mem.write_u8(0x2000, 0xFF).unwrap();
        assert_eq!(mem.read_u8(0x2001).unwrap(), 0x00);
        assert_eq!(mem.read_u8(0x2FFF).unwrap(), 0x00);
    }

    #[test]
    fn page_boundary_separates_pages() {
        let mut mem = PagedMemory::new();
        // Last byte of page 0 and first byte of page 1 are independent.
        mem.write_u8(0x0FFF, 0xAA).unwrap();
        // Page 1 (0x1000) is not yet allocated.
        assert!(mem.read_u8(0x1000).is_err());
        // After writing to page 1, page 0's data is unchanged.
        mem.write_u8(0x1000, 0xBB).unwrap();
        assert_eq!(mem.read_u8(0x0FFF).unwrap(), 0xAA);
        assert_eq!(mem.read_u8(0x1000).unwrap(), 0xBB);
    }

    #[test]
    fn writes_to_different_pages_dont_alias() {
        let mut mem = PagedMemory::new();
        mem.write_u8(0x0000, 0x11).unwrap();
        mem.write_u8(0x1000, 0x22).unwrap();
        mem.write_u8(0x2000, 0x33).unwrap();
        assert_eq!(mem.read_u8(0x0000).unwrap(), 0x11);
        assert_eq!(mem.read_u8(0x1000).unwrap(), 0x22);
        assert_eq!(mem.read_u8(0x2000).unwrap(), 0x33);
    }

    // read_u32_le / write_u32_le

    #[test]
    fn write_u32_then_read_u32_roundtrip() {
        let mut mem = PagedMemory::new();
        mem.write_u32_le(0x1000, 0xDEAD_BEEF).unwrap();
        assert_eq!(mem.read_u32_le(0x1000).unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn write_u32_uses_little_endian_byte_order() {
        let mut mem = PagedMemory::new();
        mem.write_u32_le(0x1000, 0xDEAD_BEEF).unwrap();
        // ARMv7-A: little-endian places LSB at lowest address.
        assert_eq!(mem.read_u8(0x1000).unwrap(), 0xEF);
        assert_eq!(mem.read_u8(0x1001).unwrap(), 0xBE);
        assert_eq!(mem.read_u8(0x1002).unwrap(), 0xAD);
        assert_eq!(mem.read_u8(0x1003).unwrap(), 0xDE);
    }

    #[test]
    fn read_u32_assembles_bytes_little_endian() {
        let mut mem = PagedMemory::new();
        mem.write_u8(0x1000, 0x78).unwrap();
        mem.write_u8(0x1001, 0x56).unwrap();
        mem.write_u8(0x1002, 0x34).unwrap();
        mem.write_u8(0x1003, 0x12).unwrap();
        assert_eq!(mem.read_u32_le(0x1000).unwrap(), 0x1234_5678);
    }

    #[test]
    fn read_u32_at_page_boundary_crosses_pages() {
        // The last 4 bytes of one page span into the next page when read at offset PAGE_SIZE-1.
        let mut mem = PagedMemory::new();
        mem.write_u8(0x0FFD, 0x11).unwrap();
        mem.write_u8(0x0FFE, 0x22).unwrap();
        mem.write_u8(0x0FFF, 0x33).unwrap();
        mem.write_u8(0x1000, 0x44).unwrap(); // crosses into page 1
        assert_eq!(mem.read_u32_le(0x0FFD).unwrap(), 0x4433_2211);
    }

    #[test]
    fn read_u32_unmapped_returns_translation_fault() {
        let mut mem = PagedMemory::new();
        let result = mem.read_u32_le(0x5000);
        match result {
            Err(Fault::Translation { address, is_write }) => {
                assert_eq!(address, 0x5000);
                assert!(!is_write);
            }
            _ => panic!("expected Translation fault, got {:?}", result),
        }
    }

    #[test]
    fn read_u32_cross_page_with_second_page_unmapped_faults() {
        let mut mem = PagedMemory::new();
        // Map page 0 but not page 1, then read straddling the boundary.
        mem.write_u8(0x0FFF, 0xAA).unwrap();
        let result = mem.read_u32_le(0x0FFD);
        assert!(matches!(result, Err(Fault::Translation { .. })));
    }

    // read_u16_le / write_u16_le

    #[test]
    fn write_u16_then_read_u16_roundtrip() {
        let mut mem = PagedMemory::new();
        mem.write_u16_le(0x1000, 0xCAFE).unwrap();
        assert_eq!(mem.read_u16_le(0x1000).unwrap(), 0xCAFE);
    }

    #[test]
    fn write_u16_uses_little_endian_byte_order() {
        let mut mem = PagedMemory::new();
        mem.write_u16_le(0x1000, 0xCAFE).unwrap();
        assert_eq!(mem.read_u8(0x1000).unwrap(), 0xFE);
        assert_eq!(mem.read_u8(0x1001).unwrap(), 0xCA);
    }

    #[test]
    fn read_u16_at_page_boundary_crosses_pages() {
        let mut mem = PagedMemory::new();
        mem.write_u8(0x0FFF, 0x11).unwrap();
        mem.write_u8(0x1000, 0x22).unwrap();
        assert_eq!(mem.read_u16_le(0x0FFF).unwrap(), 0x2211);
    }

    // fetch_u32 / fetch_u16

    #[test]
    fn fetch_u32_returns_little_endian_value() {
        // ARMv7-A instruction fetches are always little-endian (CPSR.E does not affect fetch).
        let mut mem = PagedMemory::new();
        mem.write_u32_le(0x8000, 0xE3A0_0005).unwrap(); // MOV r0, #5
        assert_eq!(mem.fetch_u32(0x8000).unwrap(), 0xE3A0_0005);
    }

    #[test]
    fn fetch_u16_returns_little_endian_value() {
        let mut mem = PagedMemory::new();
        mem.write_u16_le(0x8000, 0x2005).unwrap(); // Thumb: MOVS r0, #5
        assert_eq!(mem.fetch_u16(0x8000).unwrap(), 0x2005);
    }

    // load_at

    #[test]
    fn load_at_writes_bytes_in_order() {
        let mut mem = PagedMemory::new();
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        mem.load_at(0x1000, &data).unwrap();
        assert_eq!(mem.read_u8(0x1000).unwrap(), 0xDE);
        assert_eq!(mem.read_u8(0x1001).unwrap(), 0xAD);
        assert_eq!(mem.read_u8(0x1002).unwrap(), 0xBE);
        assert_eq!(mem.read_u8(0x1003).unwrap(), 0xEF);
    }

    #[test]
    fn load_at_empty_slice_is_noop() {
        let mut mem = PagedMemory::new();
        assert!(mem.load_at(0x1000, &[]).is_ok());
        // No page should have been allocated as a side effect.
        assert!(mem.read_u8(0x1000).is_err());
    }

    #[test]
    fn load_at_spans_multiple_pages() {
        let mut mem = PagedMemory::new();
        // 8-byte block that straddles three pages: starts in page 0 (last 2 bytes),
        // fills all of page 1 minus what doesn't fit... actually let's just span 2.
        let mut data = [0u8; 6];
        for (i, b) in data.iter_mut().enumerate() {
            *b = i as u8 + 1;
        }
        mem.load_at(0x0FFE, &data).unwrap();
        assert_eq!(mem.read_u8(0x0FFE).unwrap(), 1);
        assert_eq!(mem.read_u8(0x0FFF).unwrap(), 2);
        assert_eq!(mem.read_u8(0x1000).unwrap(), 3);
        assert_eq!(mem.read_u8(0x1001).unwrap(), 4);
        assert_eq!(mem.read_u8(0x1002).unwrap(), 5);
        assert_eq!(mem.read_u8(0x1003).unwrap(), 6);
    }

    #[test]
    fn load_at_full_page_fills_exactly_one_page() {
        let mut mem = PagedMemory::new();
        let data = vec![0xA5u8; PAGE_SIZE];
        mem.load_at(0x1000, &data).unwrap();
        // First and last byte of the loaded page are set.
        assert_eq!(mem.read_u8(0x1000).unwrap(), 0xA5);
        assert_eq!(mem.read_u8(0x1FFF).unwrap(), 0xA5);
        // The next page must remain unmapped.
        assert!(mem.read_u8(0x2000).is_err());
    }

    // hot page cache

    #[test]
    fn hot_page_cache_serves_correct_data_after_switch() {
        // Read page A, then page B, then page A again. All three reads must be correct,
        // confirming the hot-page cache invalidates on switch.
        let mut mem = PagedMemory::new();
        mem.write_u8(0x0000, 0xAA).unwrap();
        mem.write_u8(0x1000, 0xBB).unwrap();
        mem.write_u8(0x2000, 0xCC).unwrap();

        assert_eq!(mem.read_u8(0x0000).unwrap(), 0xAA);
        assert_eq!(mem.read_u8(0x1000).unwrap(), 0xBB);
        assert_eq!(mem.read_u8(0x2000).unwrap(), 0xCC);
        assert_eq!(mem.read_u8(0x0000).unwrap(), 0xAA); // back to page 0
    }

    #[test]
    fn repeated_reads_in_same_page_return_consistent_data() {
        let mut mem = PagedMemory::new();
        mem.write_u8(0x1000, 0x77).unwrap();
        mem.write_u8(0x1FFF, 0x88).unwrap();
        for _ in 0..5 {
            assert_eq!(mem.read_u8(0x1000).unwrap(), 0x77);
            assert_eq!(mem.read_u8(0x1FFF).unwrap(), 0x88);
        }
    }
}