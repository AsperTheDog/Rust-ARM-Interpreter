use crate::emulator::Fault;

pub mod paged_mem;

pub trait Memory {
    fn read_u8(&mut self, addr: u32) -> Result<u8, Fault>;
    fn write_u8(&mut self, addr: u32, val: u8) -> Result<(), Fault>;

    #[inline(always)]
    fn fetch_u32(&mut self, addr: u32) -> Result<u32, Fault> {
        self.read_u32_le(addr)
    }

    #[inline(always)]
    fn fetch_u16(&mut self, addr: u32) -> Result<u16, Fault> {
        self.read_u16_le(addr)
    }

    fn load_at(&mut self, addr: u32, data: &[u8]) -> Result<(), Fault> {
        for (i, &byte) in data.iter().enumerate() {
            self.write_u8(addr + i as u32, byte)?;
        }
        Ok(())
    }

    fn read_u16_le(&mut self, addr: u32) -> Result<u16, Fault> {
        Ok(u16::from_le_bytes([
            self.read_u8(addr + 1)?,
            self.read_u8(addr + 2)?
        ]))
    }

    fn read_u32_le(&mut self, addr: u32) -> Result<u32, Fault> {
        Ok(u32::from_le_bytes([
            self.read_u8(addr)?,
            self.read_u8(addr + 1)?,
            self.read_u8(addr + 2)?,
            self.read_u8(addr + 3)?,
        ]))
    }

    fn write_u16_le(&mut self, addr: u32, val: u16) -> Result<(), Fault> {
        let bytes = val.to_le_bytes();
        self.write_u8(addr, bytes[0])?;
        self.write_u8(addr + 1, bytes[1])?;
        Ok(())
    }

    fn write_u32_le(&mut self, addr: u32, val: u32) -> Result<(), Fault> {
        let bytes = val.to_le_bytes();
        for i in 0..4 {
            self.write_u8(addr + i as u32, bytes[i])?;
        }
        Ok(())
    }

    fn print_block(&mut self, start_addr: u32, end_addr: u32) {
        let mut row_start = start_addr & !0xF;

        println!("--- Memory Dump [0x{:08X} - 0x{:08X}] ---", start_addr, end_addr);
        println!(" Address  | 00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F | ASCII");
        println!("----------|-------------------------------------------------|------------------");

        while row_start < end_addr {
            print!(" {:08X} | ", row_start);

            let mut ascii_row = String::with_capacity(16);

            for i in 0..16 {
                let current_addr = row_start + i;

                if current_addr < start_addr || current_addr >= end_addr {
                    print!("   ");
                    ascii_row.push(' ');
                    continue;
                }

                match self.read_u8(current_addr) {
                    Ok(val) => {
                        print!("{:02X} ", val);
                        if (32..=126).contains(&val) {
                            ascii_row.push(val as char);
                        } else {
                            ascii_row.push('.');
                        }
                    }
                    Err(_) => {
                        print!("?? ");
                        ascii_row.push(' ');
                    }
                }
            }

            println!("| {}", ascii_row);
            row_start = row_start.wrapping_add(16);
        }
        println!();
    }
}