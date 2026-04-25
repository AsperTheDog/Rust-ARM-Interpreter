use std::fs;

mod offsets {
    pub const SIGNATURE: usize = 0x00;
    pub const ENDIANNESS: usize = 0x05;
    pub const ARCH: usize = 0x12;
    pub const ENTRYPOINT: usize = 0x18;
    pub const HEADER_OFFSET: usize = 0x1C;
    pub const HEADER_SIZE: usize = 0x2A;
    pub const HEADER_NUM: usize = 0x2C;

    pub const SEGMENT_TYPE: usize = 0x00;
    pub const FILE_OFFSET: usize = 0x04;
    pub const VIRTUAL_ADDR: usize = 0x08;
    pub const FILE_SIZE: usize = 0x10;
    pub const MEM_SIZE: usize = 0x14;
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Endian {
    Little,
    Big,
}

pub struct ElfProgram {
    pub virtual_address: u32,
    pub data: Vec<u8>,
}

pub struct ElfFile {
    pub entry_point: u32,
    pub programs: Vec<ElfProgram>,
}

impl ElfFile {
    pub fn new(file: &str) -> Result<Self, String> {
        println!("Reading file: {}", file);
        let bytes = fs::read(file).map_err(|e| format!("Error opening file: {}", e))?;

        let sig_chunk = bytes
            .get(offsets::SIGNATURE..offsets::SIGNATURE + 4)
            .ok_or_else(|| "File size too small for signature".to_string())?;

        if sig_chunk != [0x7F, b'E', b'L', b'F'] {
            return Err("Invalid signature".to_string());
        }

        let parsed_endian = match bytes.get(offsets::ENDIANNESS) {
            Some(1) => Endian::Little,
            Some(2) => Endian::Big,
            _ => return Err("Invalid or missing endianness flag".to_string()),
        };

        let read_u32 = |offset: usize, err: &str| -> Result<u32, String> {
            let chunk = bytes.get(offset..offset + 4).ok_or_else(|| err.to_string())?;
            let arr: [u8; 4] = chunk.try_into().unwrap();
            Ok(match parsed_endian {
                Endian::Little => u32::from_le_bytes(arr),
                Endian::Big => u32::from_be_bytes(arr),
            })
        };

        let read_u16 = |offset: usize, err: &str| -> Result<u16, String> {
            let chunk = bytes.get(offset..offset + 2).ok_or_else(|| err.to_string())?;
            let arr: [u8; 2] = chunk.try_into().unwrap();
            Ok(match parsed_endian {
                Endian::Little => u16::from_le_bytes(arr),
                Endian::Big => u16::from_be_bytes(arr),
            })
        };

        let arch_val = read_u16(offsets::ARCH, "Missing architecture")?;
        if arch_val != 0x28 { // ARM
            return Err(format!("Invalid arch: 0x{:02X}", arch_val));
        }

        let entry_point = read_u32(offsets::ENTRYPOINT, "Missing entry point")?;
        let header_offset = read_u32(offsets::HEADER_OFFSET, "Missing header offset")?;
        let header_size = read_u16(offsets::HEADER_SIZE, "Missing header size")?;
        let header_num = read_u16(offsets::HEADER_NUM, "Missing header num")?;

        let mut programs = Vec::with_capacity(header_num as usize);

        for i in 0..header_num {
            let h_addr = (header_offset + (i as u32 * header_size as u32)) as usize;

            let seg_type = read_u32(h_addr + offsets::SEGMENT_TYPE, "Missing segment type")?;
            if seg_type != 1 { continue; } // Only PT_LOAD

            let data_offset = read_u32(h_addr + offsets::FILE_OFFSET, "Missing data offset")?;
            let virtual_address = read_u32(h_addr + offsets::VIRTUAL_ADDR, "Missing virt addr")?;
            let data_size = read_u32(h_addr + offsets::FILE_SIZE, "Missing file size")?;
            let mem_size = read_u32(h_addr + offsets::MEM_SIZE, "Missing mem size")?;

            if mem_size < data_size {
                return Err(format!("Segment {}: mem_size < data_size", i));
            }

            let file_slice = bytes
                .get((data_offset as usize)..(data_offset + data_size) as usize)
                .ok_or_else(|| format!("Segment {}: slice out of bounds", i))?;

            let mut data = Vec::with_capacity(mem_size as usize);
            data.extend_from_slice(file_slice);
            data.resize(mem_size as usize, 0);

            programs.push(ElfProgram { virtual_address, data });
        }

        println!("File parsed succesfully");
        println!("--- ELF Summary ---");
        println!("Endianness: {:?}", parsed_endian);
        println!("Entry Point: 0x{:08X}", entry_point);
        println!("Program Headers: {} ({} loaded)", header_num, programs.len());

        for (i, p) in programs.iter().enumerate() {
            println!("  [{}] Load Address: 0x{:08X}, Size: {} bytes", i, p.virtual_address, p.data.len());
        }

        Ok(ElfFile { entry_point, programs })
    }
}
