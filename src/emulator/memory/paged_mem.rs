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