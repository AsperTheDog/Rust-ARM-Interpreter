use crate::emulator::Fault;

pub mod args;

pub fn get_bits(data: u32, bit_high: u8, bit_low: u8) -> u32 {
    let bit_range = (1u32 << ((bit_high - bit_low) + 1)) - 1;
    (data >> bit_low) & bit_range
}

pub fn get_bit(data: u32, bit: u8) -> bool {
    (data >> bit) & 1 == 1
}