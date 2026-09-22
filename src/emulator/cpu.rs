use crate::elf::reader::ElfFile;
use crate::emulator::decoder::Instruction;
use crate::emulator::Fault;
use crate::emulator::Fault::{Alignment, Unknown};
use crate::emulator::memory::Memory;

pub mod cpsr_flags {
    // Condition Flags
    pub const N: u32 = 1 << 31; // Negative
    pub const Z: u32 = 1 << 30; // Zero
    pub const C: u32 = 1 << 29; // Carry
    pub const V: u32 = 1 << 28; // Overflow
    pub const Q: u32 = 1 << 27; // Sticky Overflow (DSP)

    // Execution State Bits
    pub const J: u32 = 1 << 24; // Jazelle
    pub const E: u32 = 1 << 9;  // Endianness (0=LE, 1=BE)
    pub const A: u32 = 1 << 8;  // Mask imprecise aborts
    pub const I: u32 = 1 << 7;  // IRQ disable
    pub const F: u32 = 1 << 6;  // FIQ disable
    pub const T: u32 = 1 << 5;  // Thumb mode

    // Mode Bits
    pub const MODE_MASK: u32 = 0x1F;
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Mode {
    User       = 0b10000,
    Fiq        = 0b10001,
    Irq        = 0b10010,
    Supervisor = 0b10011,
    Abort      = 0b10111,
    Undefined  = 0b11011,
    System     = 0b11111,
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Condition {
    EQ = 0x0, // Equal (Z set)
    NE = 0x1, // Not Equal (Z clear)
    CS = 0x2, // Carry Set / Unsigned Higher or Same
    CC = 0x3, // Carry Clear / Unsigned Lower
    MI = 0x4, // Minus / Negative
    PL = 0x5, // Plus / Positive or Zero
    VS = 0x6, // Overflow Set
    VC = 0x7, // Overflow Clear
    HI = 0x8, // Unsigned Higher
    LS = 0x9, // Unsigned Lower or Same
    GE = 0xA, // Signed Greater Than or Equal
    LT = 0xB, // Signed Less Than
    GT = 0xC, // Signed Greater Than
    LE = 0xD, // Signed Less Than or Equal
    AL = 0xE, // Always (Standard instructions)
    UN = 0xF, // Unconditional instruction
}

impl Condition {
    pub fn from_u4(data: u8) -> Condition {
        match data {
            0 => Condition::EQ,
            1 => Condition::NE,
            2 => Condition::CS,
            3 => Condition::CC,
            4 => Condition::MI,
            5 => Condition::PL,
            6 => Condition::VS,
            7 => Condition::VC,
            8 => Condition::HI,
            9 => Condition::LS,
            10 => Condition::GE,
            11 => Condition::LT,
            12 => Condition::GT,
            13 => Condition::LE,
            14 => Condition::AL,
            15 => Condition::UN,
            _ => unreachable!(),
        }
    }

    pub fn thumb_check(data: u32) -> Condition {
        if (data & 0xF000) == 0xD000 {
            let cond = ((data >> 8) & 0xF) as u8;
            if cond < 0xE {
                return Condition::from_u4(cond);
            }
        }

        if (data >> 27) == 0x1E && (data & 0x0000_D000) == 0x0000_8000 {
            let cond = ((data >> 22) & 0xF) as u8;
            if (cond & 0xE) != 0xE {
                return Condition::from_u4(cond);
            }
        }

        Condition::AL
    }
}

impl Mode {
    pub fn from_bits(bits: u32) -> Result<Self, Fault> {
        match bits & cpsr_flags::MODE_MASK {
            0b10000 => Ok(Mode::User),
            0b10001 => Ok(Mode::Fiq),
            0b10010 => Ok(Mode::Irq),
            0b10011 => Ok(Mode::Supervisor),
            0b10111 => Ok(Mode::Abort),
            0b11011 => Ok(Mode::Undefined),
            0b11111 => Ok(Mode::System),
            _ => Err(Unknown{}),
        }
    }
}

pub struct Cpsr {
    val: u32
}

impl Cpsr {
    #[inline(always)]
    pub fn get_cpsr_bit(&self, mask: u32) -> bool {
        (self.val & mask) != 0
    }

    #[inline(always)]
    pub fn set_cpsr_bit(&mut self, mask: u32, value: bool) {
        if value {
            self.val |= mask;
        } else {
            self.val &= !mask;
        }
    }

    pub fn get_flags(&self) -> (bool, bool, bool, bool) {
        (
            self.get_cpsr_bit(cpsr_flags::N),
            self.get_cpsr_bit(cpsr_flags::Z),
            self.get_cpsr_bit(cpsr_flags::C),
            self.get_cpsr_bit(cpsr_flags::V),
        )
    }

    pub fn is_thumb(&self) -> bool {
        self.get_cpsr_bit(cpsr_flags::T)
    }

    pub fn get_itstate(&self) -> u8 {
        let it_7_2 = (self.val >> 10) & 0x3F; // Get 6 bits
        let it_1_0 = (self.val >> 25) & 0x03; // Get 2 bits

        ((it_7_2 << 2) | it_1_0) as u8
    }

    pub fn set_itstate(&mut self, value: u8) {
        let it_7_2 = ((value >> 2) & 0x3F) as u32;
        let it_1_0 = (value & 0x03) as u32;

        self.val &= !((0x3F << 10) | (0x03 << 25));
        self.val |= (it_7_2 << 10) | (it_1_0 << 25);
    }

    pub fn advance_itstate(&mut self) {
        let it = self.get_itstate();
        if it == 0 { return; }

        if (it & 0b111) == 0 {
            self.set_itstate(0);
        } else {
            let top_3 = it & 0xE0;
            let shift_part = it & 0x1F;
            let new_shift = (shift_part << 1) & 0x1F;

            self.set_itstate(top_3 | new_shift);
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.val = (self.val & !cpsr_flags::MODE_MASK) | (mode as u32);
    }

    pub fn update_nz_flags(&mut self, result: u32) {
        self.set_cpsr_bit(cpsr_flags::N, (result as i32) < 0);
        self.set_cpsr_bit(cpsr_flags::Z, result == 0);
    }

    pub fn add_with_flags(&mut self, a: u32, b: u32, carry_in: bool, update_flags: bool) -> u32 {
        let op1 = a as u64;
        let op2 = b as u64;
        let cin = if carry_in { 1 } else { 0 };

        let full_result = op1 + op2 + cin;
        let result = full_result as u32;

        if update_flags {
            self.set_cpsr_bit(cpsr_flags::N, (result as i32) < 0);
            self.set_cpsr_bit(cpsr_flags::Z, result == 0);
            self.set_cpsr_bit(cpsr_flags::C, (full_result >> 32) != 0);
            self.set_cpsr_bit(cpsr_flags::V, ((a ^ result) & (b ^ result)) >> 31 != 0);
        }

        result
    }

    pub fn sub_with_flags(&mut self, a: u32, b: u32, carry_in: bool, update_flags: bool) -> u32 {
        let cin = if carry_in { 1 } else { 0 };

        let op1 = a as u64;
        let op2 = (!b) as u64;

        let full_result = op1 + op2 + (cin as u64);
        let result = full_result as u32;

        if update_flags {
            self.set_cpsr_bit(cpsr_flags::N, (result as i32) < 0);
            self.set_cpsr_bit(cpsr_flags::Z, result == 0);
            self.set_cpsr_bit(cpsr_flags::C, (full_result >> 32) != 0);
            self.set_cpsr_bit(cpsr_flags::V, ((a ^ b) & (a ^ result)) >> 31 != 0);
        }

        result
    }
}

pub struct Cpu<M: Memory> {
    registers: [u32; 16],
    pub cpsr: Cpsr,

    pub memory: M,

    pub halted: bool,
    pub pc_dirty: bool
}

impl <M: Memory> Cpu<M> {
    pub fn new(memory: M) -> Self {
        Cpu{ registers: [0; 16], cpsr: Cpsr{ val: 0 }, memory, halted: false, pc_dirty: false }
    }

    pub fn load_elf(&mut self, elf: ElfFile) -> Result<(), Fault> {
        self.registers[15] = elf.entry_point;

        for segment in elf.programs {
            println!("Loading segment at 0x{:08X} ({} bytes)", segment.virtual_address, segment.data.len());
            self.memory.load_at(segment.virtual_address, &segment.data)?;
        }

        self.registers[13] = 0x80000000;
        Ok(())
    }

    pub fn fetch(&mut self) -> Result<(u32, u32), Fault> {
        let pc = self.registers[15];
        if self.cpsr.get_cpsr_bit(cpsr_flags::T)
        {
            if !pc.is_multiple_of(2) {
                return Err(Alignment { address: pc });
            }

            let mut instruction = self.memory.fetch_u16(pc)? as u32;

            let instr_size: u32;
            if Instruction::is_wide_op(instruction) {
                let instruction2 = self.memory.fetch_u16(pc + 2)? as u32;

                instruction = (instruction << 16) | (instruction2);

                instr_size = 4;
            }
            else {
                instr_size = 2;
            }

            Ok((instruction, instr_size))
        }
        else
        {
            if !pc.is_multiple_of(4) {
                return Err(Alignment { address: pc });
            }

            let instruction = self.memory.fetch_u32(pc)?;

            Ok((instruction, 4))
        }
    }

    pub fn meets_condition(&self, cond: Condition) -> bool {
        let (n, z, c, v) = self.cpsr.get_flags();

        match cond {
            Condition::EQ => z,
            Condition::NE => !z,
            Condition::CS => c,
            Condition::CC => !c,
            Condition::MI => n,
            Condition::PL => !n,
            Condition::VS => v,
            Condition::VC => !v,
            Condition::HI => c && !z,
            Condition::LS => !c || z,
            Condition::GE => n == v,
            Condition::LT => n != v,
            Condition::GT => !z && (n == v),
            Condition::LE => z || (n != v),
            Condition::AL => true,
            Condition::UN => panic!(),
        }
    }

    pub fn decode_instruction(&self, data: u32) -> Result<Instruction, Fault> {
        let thumb_mode: bool = self.cpsr.is_thumb();
        let cond = if thumb_mode {
            let it = self.cpsr.get_itstate();
            if it != 0 {
                Condition::from_u4(it >> 4)
            } else {
                Condition::thumb_check(data)
            }
        } else {
            let cond_bits = (data >> 28) as u8;
            Condition::from_u4(cond_bits)
        };

        if cond == Condition::UN {
            return Instruction::new_unconditional(data);
        }
        else if !self.meets_condition(cond) {
            return Ok(Instruction::CondFailed);
        }

        if thumb_mode {
            Instruction::new_thumb(data)
        }
        else {
            Instruction::new_arm(data)
        }

    }

    pub fn execute(&mut self, instr: &Instruction) -> Result<(), Fault> {
        todo!()
    }

    pub fn step(&mut self) -> Result<(), Fault> {
        let (raw, instr_size) = self.fetch()?;
        let instr = self.decode_instruction(raw)?;
        self.execute(&instr)?;

        if self.cpsr.is_thumb() {
            self.cpsr.advance_itstate();
        }

        if !self.pc_dirty {
            self.registers[15] = self.registers[15].wrapping_add(instr_size);
        }
        self.pc_dirty = false;

        Ok(())
    }

    pub fn run(&mut self) -> Result<u32, Fault> {
        while !self.halted {
            self.step()?;
        }
        Ok(self.registers[0])
    }

    pub fn read_reg(&self, index: u8) -> u32 {
        if index == 15 {
            let offset = if self.cpsr.get_cpsr_bit(cpsr_flags::T) { 4 } else { 8 };
            self.registers[15].wrapping_add(offset)
        } else {
            self.registers[index as usize]
        }
    }

    pub fn read_reg_aligned(&self, index: u8) -> u32 {
        let val = self.read_reg(index);
        if index == 15 {
            val & !0x3
        } else {
            val
        }
    }

    pub fn write_reg(&mut self, index: u8, val: u32) {
        if index == 15 {
            self.pc_dirty = true;
        }
        self.registers[index as usize] = val;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::reader::{ElfFile, ElfProgram};
    use crate::emulator::memory::paged_mem::PagedMemory;

    fn new_cpu() -> Cpu<PagedMemory> {
        Cpu::new(PagedMemory::new())
    }

    fn cpsr() -> Cpsr {
        Cpsr { val: 0 }
    }

    // Mode::from_bits — ARMv7-A B1.3.1, Table B1-1

    #[test]
    fn mode_from_bits_recognizes_each_valid_mode() {
        assert_eq!(Mode::from_bits(0b10000).unwrap(), Mode::User);
        assert_eq!(Mode::from_bits(0b10001).unwrap(), Mode::Fiq);
        assert_eq!(Mode::from_bits(0b10010).unwrap(), Mode::Irq);
        assert_eq!(Mode::from_bits(0b10011).unwrap(), Mode::Supervisor);
        assert_eq!(Mode::from_bits(0b10111).unwrap(), Mode::Abort);
        assert_eq!(Mode::from_bits(0b11011).unwrap(), Mode::Undefined);
        assert_eq!(Mode::from_bits(0b11111).unwrap(), Mode::System);
    }

    #[test]
    fn mode_from_bits_ignores_high_bits() {
        // Only the bottom 5 bits should be examined (MODE_MASK).
        assert_eq!(Mode::from_bits(0xFFFF_FFF0).unwrap(), Mode::User);
    }

    #[test]
    fn mode_from_bits_rejects_unknown_value() {
        // 0b00000 isn't a defined ARMv7-A mode encoding.
        assert!(Mode::from_bits(0b00000).is_err());
        // 0b10100 is also unallocated.
        assert!(Mode::from_bits(0b10100).is_err());
    }

    // Condition::from_u4 — ARMv7-A A8.3, Table A8-1

    #[test]
    fn condition_from_u4_maps_each_code() {
        assert_eq!(Condition::from_u4(0x0), Condition::EQ);
        assert_eq!(Condition::from_u4(0x1), Condition::NE);
        assert_eq!(Condition::from_u4(0x2), Condition::CS);
        assert_eq!(Condition::from_u4(0x3), Condition::CC);
        assert_eq!(Condition::from_u4(0x4), Condition::MI);
        assert_eq!(Condition::from_u4(0x5), Condition::PL);
        assert_eq!(Condition::from_u4(0x6), Condition::VS);
        assert_eq!(Condition::from_u4(0x7), Condition::VC);
        assert_eq!(Condition::from_u4(0x8), Condition::HI);
        assert_eq!(Condition::from_u4(0x9), Condition::LS);
        assert_eq!(Condition::from_u4(0xA), Condition::GE);
        assert_eq!(Condition::from_u4(0xB), Condition::LT);
        assert_eq!(Condition::from_u4(0xC), Condition::GT);
        assert_eq!(Condition::from_u4(0xD), Condition::LE);
        assert_eq!(Condition::from_u4(0xE), Condition::AL);
        assert_eq!(Condition::from_u4(0xF), Condition::UN);
    }

    // Condition::thumb_check — ARMv7-A A8.8.18 B encodings T1, T3

    #[test]
    fn thumb_check_recognizes_16bit_b_cond() {
        // T1 encoding: 1101 cond imm8. cond=0..0xD are valid.
        assert_eq!(Condition::thumb_check(0xD000), Condition::EQ);
        assert_eq!(Condition::thumb_check(0xD1FF), Condition::NE);
        assert_eq!(Condition::thumb_check(0xDC00), Condition::GT);
        assert_eq!(Condition::thumb_check(0xDD00), Condition::LE);
    }

    #[test]
    fn thumb_check_16bit_cond_0xe_is_not_a_conditional_branch() {
        // cond=0b1110 in T1 is the UDF permanently-undefined slot, not B<c>.
        // The decoder treats only cond<0xE as conditional, so default AL.
        assert_eq!(Condition::thumb_check(0xDE00), Condition::AL);
    }

    #[test]
    fn thumb_check_16bit_cond_0xf_is_svc_not_conditional() {
        // cond=0b1111 in T1 is SVC, not B<c>.
        assert_eq!(Condition::thumb_check(0xDF00), Condition::AL);
    }

    #[test]
    fn thumb_check_non_branch_16bit_returns_al() {
        // Random non-1101-prefixed Thumb instruction (e.g. MOVS r0, #0).
        assert_eq!(Condition::thumb_check(0x2000), Condition::AL);
    }

    #[test]
    fn thumb_check_recognizes_32bit_b_cond_w() {
        // T3 encoding: hw1 = 11110 S cond imm6,  hw2 = 10 J1 0 J2 imm11.
        // Build B EQ.W: cond=0, S=0, hw2[12]=0. hw1=0xF000, hw2=0x8000.
        let data = (0xF000u32 << 16) | 0x8000u32;
        assert_eq!(Condition::thumb_check(data), Condition::EQ);

        // B GT.W: cond=0xC. hw1[9:6]=1100 → hw1=0xF300.
        let data = (0xF300u32 << 16) | 0x8000u32;
        assert_eq!(Condition::thumb_check(data), Condition::GT);
    }

    #[test]
    fn thumb_check_32bit_cond_0xe_not_treated_as_conditional() {
        // cond bits of 0b111x are reserved in T3 (would be unconditional B);
        // implementation guards with (cond & 0xE) != 0xE so it falls through to AL.
        let data = (0xF380u32 << 16) | 0x8000u32; // cond=0b1110
        assert_eq!(Condition::thumb_check(data), Condition::AL);
    }

    #[test]
    fn thumb_check_32bit_unconditional_b_w_is_not_conditional() {
        // T4 encoding: hw2[12]=1 (instead of 0). hw2=0x9000 distinguishes it from T3.
        // The bits where T3 would put cond are part of imm10 here, so we must NOT
        // pull a "condition" out of them.
        let data = (0xF000u32 << 16) | 0x9000u32;
        assert_eq!(Condition::thumb_check(data), Condition::AL);
    }

    // Cpsr flag accessors — ARMv7-A B1.3.3, CPSR layout

    #[test]
    fn cpsr_set_and_get_each_flag() {
        let mut c = cpsr();
        for &flag in &[
            cpsr_flags::N, cpsr_flags::Z, cpsr_flags::C, cpsr_flags::V,
            cpsr_flags::Q, cpsr_flags::J, cpsr_flags::E, cpsr_flags::A,
            cpsr_flags::I, cpsr_flags::F, cpsr_flags::T,
        ] {
            assert!(!c.get_cpsr_bit(flag));
            c.set_cpsr_bit(flag, true);
            assert!(c.get_cpsr_bit(flag));
            c.set_cpsr_bit(flag, false);
            assert!(!c.get_cpsr_bit(flag));
        }
    }

    #[test]
    fn cpsr_flags_are_independent() {
        let mut c = cpsr();
        c.set_cpsr_bit(cpsr_flags::N, true);
        c.set_cpsr_bit(cpsr_flags::C, true);
        c.set_cpsr_bit(cpsr_flags::Z, true);
        c.set_cpsr_bit(cpsr_flags::Z, false); // clearing Z must not affect N or C
        assert!(c.get_cpsr_bit(cpsr_flags::N));
        assert!(!c.get_cpsr_bit(cpsr_flags::Z));
        assert!(c.get_cpsr_bit(cpsr_flags::C));
    }

    #[test]
    fn cpsr_get_flags_returns_nzcv_in_order() {
        let mut c = cpsr();
        c.set_cpsr_bit(cpsr_flags::N, true);
        c.set_cpsr_bit(cpsr_flags::V, true);
        assert_eq!(c.get_flags(), (true, false, false, true));
    }

    #[test]
    fn cpsr_is_thumb_tracks_t_bit() {
        let mut c = cpsr();
        assert!(!c.is_thumb());
        c.set_cpsr_bit(cpsr_flags::T, true);
        assert!(c.is_thumb());
    }

    #[test]
    fn cpsr_set_mode_replaces_only_mode_bits() {
        let mut c = cpsr();
        // Pre-load some non-mode bits to prove they're preserved.
        c.set_cpsr_bit(cpsr_flags::N, true);
        c.set_cpsr_bit(cpsr_flags::T, true);
        c.set_mode(Mode::Supervisor);
        assert_eq!(c.val & cpsr_flags::MODE_MASK, Mode::Supervisor as u32);
        assert!(c.get_cpsr_bit(cpsr_flags::N));
        assert!(c.get_cpsr_bit(cpsr_flags::T));
        // Switching modes overwrites the previous mode field, doesn't OR-in.
        c.set_mode(Mode::User);
        assert_eq!(c.val & cpsr_flags::MODE_MASK, Mode::User as u32);
    }

    #[test]
    fn cpsr_update_nz_flags_zero_result() {
        let mut c = cpsr();
        c.set_cpsr_bit(cpsr_flags::N, true); // dirty
        c.update_nz_flags(0);
        assert!(!c.get_cpsr_bit(cpsr_flags::N));
        assert!(c.get_cpsr_bit(cpsr_flags::Z));
    }

    #[test]
    fn cpsr_update_nz_flags_negative_result() {
        let mut c = cpsr();
        c.update_nz_flags(0x8000_0000);
        assert!(c.get_cpsr_bit(cpsr_flags::N));
        assert!(!c.get_cpsr_bit(cpsr_flags::Z));
    }

    #[test]
    fn cpsr_update_nz_flags_positive_result() {
        let mut c = cpsr();
        c.set_cpsr_bit(cpsr_flags::N, true); // dirty
        c.set_cpsr_bit(cpsr_flags::Z, true); // dirty
        c.update_nz_flags(1);
        assert!(!c.get_cpsr_bit(cpsr_flags::N));
        assert!(!c.get_cpsr_bit(cpsr_flags::Z));
    }

    // Cpsr IT state — ARMv7-A A2.5.2 / B1.4.5
    // ITSTATE is split: bits[7:2] live in CPSR[15:10], bits[1:0] in CPSR[26:25].

    #[test]
    fn cpsr_itstate_roundtrip_low_bits() {
        // Value 0b00000011 exercises only the low two ITSTATE bits (CPSR[26:25]).
        let mut c = cpsr();
        c.set_itstate(0b0000_0011);
        assert_eq!(c.get_itstate(), 0b0000_0011);
    }

    #[test]
    fn cpsr_itstate_roundtrip_high_bits() {
        // Value 0b1111_1100 exercises only the upper six ITSTATE bits (CPSR[15:10]).
        let mut c = cpsr();
        c.set_itstate(0b1111_1100);
        assert_eq!(c.get_itstate(), 0b1111_1100);
    }

    #[test]
    fn cpsr_itstate_roundtrip_full_byte() {
        let mut c = cpsr();
        c.set_itstate(0xAB);
        assert_eq!(c.get_itstate(), 0xAB);
    }

    #[test]
    fn cpsr_itstate_set_does_not_disturb_other_cpsr_bits() {
        let mut c = cpsr();
        c.set_cpsr_bit(cpsr_flags::N, true);
        c.set_cpsr_bit(cpsr_flags::T, true);
        c.set_cpsr_bit(cpsr_flags::Q, true);
        c.set_itstate(0xFF);
        assert!(c.get_cpsr_bit(cpsr_flags::N));
        assert!(c.get_cpsr_bit(cpsr_flags::T));
        assert!(c.get_cpsr_bit(cpsr_flags::Q));
        // And clearing IT state should leave them alone too.
        c.set_itstate(0);
        assert!(c.get_cpsr_bit(cpsr_flags::N));
        assert!(c.get_cpsr_bit(cpsr_flags::T));
        assert!(c.get_cpsr_bit(cpsr_flags::Q));
    }

    #[test]
    fn cpsr_advance_itstate_zero_stays_zero() {
        let mut c = cpsr();
        c.advance_itstate();
        assert_eq!(c.get_itstate(), 0);
    }

    #[test]
    fn cpsr_advance_itstate_clears_when_bottom_three_bits_zero() {
        // Per ARM ARM ITAdvance(): if ITSTATE<2:0> == '000' then ITSTATE = '00000000'.
        // Pick a value whose bottom 3 bits are zero but which is non-zero overall.
        let mut c = cpsr();
        c.set_itstate(0b1010_1000);
        c.advance_itstate();
        assert_eq!(c.get_itstate(), 0);
    }

    #[test]
    fn cpsr_advance_itstate_shifts_low_5_bits_left() {
        // Per ARM ARM ITAdvance(): else ITSTATE<4:0> = LSL(ITSTATE<4:0>, 1).
        // top 3 bits are the firstcond[3:1] base and must be preserved.
        let mut c = cpsr();
        // 0b1011_1001: top3=101, low5=11001. Shift low5 left → 10010.
        c.set_itstate(0b1011_1001);
        c.advance_itstate();
        assert_eq!(c.get_itstate(), 0b1011_0010);
    }

    // Cpsr::add_with_flags — ARMv7-A A2.2.1 AddWithCarry

    #[test]
    fn add_with_flags_simple_no_overflow() {
        let mut c = cpsr();
        let r = c.add_with_flags(2, 3, false, true);
        assert_eq!(r, 5);
        let (n, z, ci, v) = c.get_flags();
        assert!(!n && !z && !ci && !v);
    }

    #[test]
    fn add_with_flags_zero_result_sets_z() {
        let mut c = cpsr();
        c.add_with_flags(0, 0, false, true);
        let (n, z, _, v) = c.get_flags();
        assert!(!n && z && !v);
    }

    #[test]
    fn add_with_flags_negative_result_sets_n() {
        let mut c = cpsr();
        c.add_with_flags(0x7FFF_FFFE, 1, false, true); // 0x7FFFFFFF, still positive
        assert!(!c.get_cpsr_bit(cpsr_flags::N));
        c.add_with_flags(0x7FFF_FFFF, 1, false, true); // 0x80000000 → negative + V
        assert!(c.get_cpsr_bit(cpsr_flags::N));
    }

    #[test]
    fn add_with_flags_unsigned_overflow_sets_c() {
        // 0xFFFFFFFF + 1 = 0x1_0000_0000, wraps to 0; carry out.
        let mut c = cpsr();
        let r = c.add_with_flags(0xFFFF_FFFF, 1, false, true);
        assert_eq!(r, 0);
        assert!(c.get_cpsr_bit(cpsr_flags::C));
        assert!(c.get_cpsr_bit(cpsr_flags::Z));
        assert!(!c.get_cpsr_bit(cpsr_flags::V)); // not signed overflow
    }

    #[test]
    fn add_with_flags_signed_overflow_sets_v() {
        // 0x7FFFFFFF + 1 = 0x80000000: signed overflow (positive + positive → negative).
        let mut c = cpsr();
        c.add_with_flags(0x7FFF_FFFF, 1, false, true);
        assert!(c.get_cpsr_bit(cpsr_flags::V));
        assert!(c.get_cpsr_bit(cpsr_flags::N));
        assert!(!c.get_cpsr_bit(cpsr_flags::C));
    }

    #[test]
    fn add_with_flags_carry_in_acts_as_plus_one() {
        // Used by ADC: result = a + b + carry_in.
        let mut c = cpsr();
        let r = c.add_with_flags(2, 3, true, false);
        assert_eq!(r, 6);
    }

    #[test]
    fn add_with_flags_no_update_leaves_flags_alone() {
        let mut c = cpsr();
        c.set_cpsr_bit(cpsr_flags::N, true);
        c.set_cpsr_bit(cpsr_flags::Z, true);
        c.set_cpsr_bit(cpsr_flags::C, true);
        c.set_cpsr_bit(cpsr_flags::V, true);
        c.add_with_flags(1, 1, false, false);
        let (n, z, ci, v) = c.get_flags();
        assert!(n && z && ci && v);
    }

    // Cpsr::sub_with_flags — ARMv7-A A2.2.1, SUB = a + NOT(b) + 1

    #[test]
    fn sub_with_flags_basic() {
        // SUB = AddWithCarry(a, NOT b, '1'). Caller passes carry_in=true for plain SUB.
        let mut c = cpsr();
        let r = c.sub_with_flags(10, 3, true, true);
        assert_eq!(r, 7);
        // No borrow → C set (ARM convention: C=1 means no borrow on SUB).
        assert!(c.get_cpsr_bit(cpsr_flags::C));
        assert!(!c.get_cpsr_bit(cpsr_flags::Z));
        assert!(!c.get_cpsr_bit(cpsr_flags::N));
        assert!(!c.get_cpsr_bit(cpsr_flags::V));
    }

    #[test]
    fn sub_with_flags_equal_operands_set_z_and_c() {
        let mut c = cpsr();
        let r = c.sub_with_flags(7, 7, true, true);
        assert_eq!(r, 0);
        assert!(c.get_cpsr_bit(cpsr_flags::Z));
        assert!(c.get_cpsr_bit(cpsr_flags::C)); // no borrow
    }

    #[test]
    fn sub_with_flags_borrow_clears_c() {
        // 3 - 5: unsigned underflow → ARM clears C (= "borrow occurred").
        let mut c = cpsr();
        c.sub_with_flags(3, 5, true, true);
        assert!(!c.get_cpsr_bit(cpsr_flags::C));
        assert!(c.get_cpsr_bit(cpsr_flags::N)); // result is negative (0xFFFFFFFE)
    }

    #[test]
    fn sub_with_flags_signed_overflow_sets_v() {
        // 0x80000000 (INT_MIN) - 1 = 0x7FFFFFFF: signed underflow.
        let mut c = cpsr();
        c.sub_with_flags(0x8000_0000, 1, true, true);
        assert!(c.get_cpsr_bit(cpsr_flags::V));
        assert!(!c.get_cpsr_bit(cpsr_flags::N));
    }

    #[test]
    fn sub_with_flags_no_update() {
        let mut c = cpsr();
        c.set_cpsr_bit(cpsr_flags::Z, true);
        c.sub_with_flags(10, 3, true, false);
        assert!(c.get_cpsr_bit(cpsr_flags::Z)); // unchanged
    }

    // Cpu register file — ARMv7-A A2.3 / A8.1.1

    #[test]
    fn write_then_read_general_register() {
        let mut cpu = new_cpu();
        cpu.write_reg(5, 0xDEAD_BEEF);
        assert_eq!(cpu.read_reg(5), 0xDEAD_BEEF);
    }

    #[test]
    fn read_pc_in_arm_mode_returns_pc_plus_8() {
        // ARMv7-A A8.1.1.2: when PC is read as data, the value is the address of
        // the current instruction + 8 in ARM state.
        let mut cpu = new_cpu();
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false; // clear side effect of write_reg
        assert_eq!(cpu.read_reg(15), 0x1008);
    }

    #[test]
    fn read_pc_in_thumb_mode_returns_pc_plus_4() {
        // Same rule, but +4 in Thumb state.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false;
        assert_eq!(cpu.read_reg(15), 0x1004);
    }

    #[test]
    fn read_reg_aligned_masks_pc_low_bits() {
        // For PC-relative load addressing, PC is read word-aligned.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.write_reg(15, 0x1002); // halfword aligned but not word aligned
        cpu.pc_dirty = false;
        // Thumb: read_reg returns 0x1002 + 4 = 0x1006; aligned masks to 0x1004.
        assert_eq!(cpu.read_reg_aligned(15), 0x1004);
    }

    #[test]
    fn read_reg_aligned_does_not_mask_general_registers() {
        let mut cpu = new_cpu();
        cpu.write_reg(0, 0x1003);
        assert_eq!(cpu.read_reg_aligned(0), 0x1003);
    }

    #[test]
    fn write_reg_15_marks_pc_dirty() {
        let mut cpu = new_cpu();
        assert!(!cpu.pc_dirty);
        cpu.write_reg(15, 0x1000);
        assert!(cpu.pc_dirty);
    }

    #[test]
    fn write_reg_non_pc_does_not_mark_pc_dirty() {
        let mut cpu = new_cpu();
        cpu.write_reg(0, 0xAA);
        cpu.write_reg(14, 0xBB); // LR
        assert!(!cpu.pc_dirty);
    }

    // Cpu::fetch — ARMv7-A A3.3 / A8.1.1

    #[test]
    fn fetch_arm_instruction_returns_word_and_size_4() {
        let mut cpu = new_cpu();
        // Place MOV r0, #5 (E3A0_0005) at 0x1000.
        cpu.memory.write_u32_le(0x1000, 0xE3A0_0005).unwrap();
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false;

        let (raw, size) = cpu.fetch().unwrap();
        assert_eq!(raw, 0xE3A0_0005);
        assert_eq!(size, 4);
    }

    #[test]
    fn fetch_arm_misaligned_pc_faults() {
        // ARMv7-A: ARM-state PC must be word-aligned, else PC alignment fault.
        let mut cpu = new_cpu();
        cpu.memory.write_u32_le(0x1000, 0xE3A0_0005).unwrap();
        cpu.write_reg(15, 0x1002);
        cpu.pc_dirty = false;
        assert!(matches!(cpu.fetch(), Err(Alignment { address: 0x1002 })));
    }

    #[test]
    fn fetch_arm_unmapped_pc_translation_fault() {
        let mut cpu = new_cpu();
        cpu.write_reg(15, 0x4000);
        cpu.pc_dirty = false;
        assert!(matches!(cpu.fetch(), Err(Fault::Translation { .. })));
    }

    #[test]
    fn fetch_thumb_narrow_instruction_returns_halfword_and_size_2() {
        // Thumb MOVS r0, #5 = 0x2005 (T1).
        let mut cpu = new_cpu();
        cpu.memory.write_u16_le(0x1000, 0x2005).unwrap();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false;

        let (raw, size) = cpu.fetch().unwrap();
        assert_eq!(raw, 0x0000_2005);
        assert_eq!(size, 2);
    }

    #[test]
    fn fetch_thumb_wide_instruction_combines_two_halfwords() {
        // 32-bit Thumb encodings start with hw1[15:11] in {0b11101, 0b11110, 0b11111}.
        // Use a B<c>.W (T3) encoding so we know it is a real wide instruction.
        // hw1 = 0xF000 (cond=EQ), hw2 = 0x8000.
        let mut cpu = new_cpu();
        cpu.memory.write_u16_le(0x1000, 0xF000).unwrap();
        cpu.memory.write_u16_le(0x1002, 0x8000).unwrap();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false;

        let (raw, size) = cpu.fetch().unwrap();
        assert_eq!(raw, 0xF000_8000);
        assert_eq!(size, 4);
    }

    #[test]
    fn fetch_thumb_misaligned_pc_faults() {
        // Thumb-state PC must be halfword-aligned.
        let mut cpu = new_cpu();
        cpu.memory.write_u16_le(0x1000, 0x2005).unwrap();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.write_reg(15, 0x1001);
        cpu.pc_dirty = false;
        assert!(matches!(cpu.fetch(), Err(Alignment { address: 0x1001 })));
    }

    // Cpu::meets_condition — ARMv7-A A8.3, Table A8-1

    fn cpu_with_flags(n: bool, z: bool, c: bool, v: bool) -> Cpu<PagedMemory> {
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::N, n);
        cpu.cpsr.set_cpsr_bit(cpsr_flags::Z, z);
        cpu.cpsr.set_cpsr_bit(cpsr_flags::C, c);
        cpu.cpsr.set_cpsr_bit(cpsr_flags::V, v);
        cpu
    }

    #[test]
    fn cond_eq_ne_track_z() {
        assert!(cpu_with_flags(false, true, false, false).meets_condition(Condition::EQ));
        assert!(!cpu_with_flags(false, false, false, false).meets_condition(Condition::EQ));
        assert!(cpu_with_flags(false, false, false, false).meets_condition(Condition::NE));
        assert!(!cpu_with_flags(false, true, false, false).meets_condition(Condition::NE));
    }

    #[test]
    fn cond_cs_cc_track_c() {
        assert!(cpu_with_flags(false, false, true, false).meets_condition(Condition::CS));
        assert!(!cpu_with_flags(false, false, false, false).meets_condition(Condition::CS));
        assert!(cpu_with_flags(false, false, false, false).meets_condition(Condition::CC));
        assert!(!cpu_with_flags(false, false, true, false).meets_condition(Condition::CC));
    }

    #[test]
    fn cond_mi_pl_track_n() {
        assert!(cpu_with_flags(true, false, false, false).meets_condition(Condition::MI));
        assert!(!cpu_with_flags(false, false, false, false).meets_condition(Condition::MI));
        assert!(cpu_with_flags(false, false, false, false).meets_condition(Condition::PL));
        assert!(!cpu_with_flags(true, false, false, false).meets_condition(Condition::PL));
    }

    #[test]
    fn cond_vs_vc_track_v() {
        assert!(cpu_with_flags(false, false, false, true).meets_condition(Condition::VS));
        assert!(!cpu_with_flags(false, false, false, false).meets_condition(Condition::VS));
        assert!(cpu_with_flags(false, false, false, false).meets_condition(Condition::VC));
        assert!(!cpu_with_flags(false, false, false, true).meets_condition(Condition::VC));
    }

    #[test]
    fn cond_hi_requires_c_set_and_z_clear() {
        // C=1 && Z=0
        assert!(cpu_with_flags(false, false, true, false).meets_condition(Condition::HI));
        assert!(!cpu_with_flags(false, true, true, false).meets_condition(Condition::HI));
        assert!(!cpu_with_flags(false, false, false, false).meets_condition(Condition::HI));
    }

    #[test]
    fn cond_ls_requires_c_clear_or_z_set() {
        assert!(cpu_with_flags(false, false, false, false).meets_condition(Condition::LS));
        assert!(cpu_with_flags(false, true, true, false).meets_condition(Condition::LS));
        assert!(!cpu_with_flags(false, false, true, false).meets_condition(Condition::LS));
    }

    #[test]
    fn cond_ge_lt_compare_n_and_v() {
        // GE: N==V
        assert!(cpu_with_flags(false, false, false, false).meets_condition(Condition::GE));
        assert!(cpu_with_flags(true, false, false, true).meets_condition(Condition::GE));
        assert!(!cpu_with_flags(true, false, false, false).meets_condition(Condition::GE));
        // LT: N!=V
        assert!(cpu_with_flags(true, false, false, false).meets_condition(Condition::LT));
        assert!(!cpu_with_flags(true, false, false, true).meets_condition(Condition::LT));
    }

    #[test]
    fn cond_gt_le_combine_z_with_n_v() {
        // GT: Z==0 && N==V
        assert!(cpu_with_flags(false, false, false, false).meets_condition(Condition::GT));
        assert!(!cpu_with_flags(false, true, false, false).meets_condition(Condition::GT));
        assert!(!cpu_with_flags(true, false, false, false).meets_condition(Condition::GT));
        // LE: Z==1 || N!=V
        assert!(cpu_with_flags(false, true, false, false).meets_condition(Condition::LE));
        assert!(cpu_with_flags(true, false, false, false).meets_condition(Condition::LE));
        assert!(!cpu_with_flags(false, false, false, false).meets_condition(Condition::LE));
    }

    #[test]
    fn cond_al_always_true() {
        // All 16 flag combinations.
        for bits in 0..16 {
            let n = bits & 1 != 0;
            let z = bits & 2 != 0;
            let c = bits & 4 != 0;
            let v = bits & 8 != 0;
            assert!(cpu_with_flags(n, z, c, v).meets_condition(Condition::AL));
        }
    }

    // Cpu::meets_condition UN — ARMv7-A A8.3
    // The 0xF code is reserved for unconditional instructions on ARMv7; calling
    // meets_condition with UN is a contract violation and panics by design.

    #[test]
    #[should_panic]
    fn meets_condition_un_panics_by_design() {
        new_cpu().meets_condition(Condition::UN);
    }

    // Cpu::new — initial state

    #[test]
    fn new_initializes_all_registers_to_zero() {
        let cpu = new_cpu();
        for r in 0..16 {
            assert_eq!(cpu.registers[r], 0, "R{} not zero on construction", r);
        }
    }

    #[test]
    fn new_initializes_cpsr_to_zero() {
        let cpu = new_cpu();
        assert_eq!(cpu.cpsr.val, 0);
    }

    #[test]
    fn new_starts_unhalted_with_clean_pc() {
        let cpu = new_cpu();
        assert!(!cpu.halted);
        assert!(!cpu.pc_dirty);
    }

    // Cpu::load_elf

    #[test]
    fn load_elf_sets_pc_to_entry_point() {
        let mut cpu = new_cpu();
        cpu.load_elf(ElfFile { entry_point: 0x8000, programs: vec![] }).unwrap();
        assert_eq!(cpu.registers[15], 0x8000);
    }

    #[test]
    fn load_elf_sets_sp_to_top_of_address_space() {
        // ARMv7-A doesn't mandate a specific SP — this asserts the emulator's chosen
        // initial SP of 0x80000000 (full descending stack at the 2GiB boundary).
        let mut cpu = new_cpu();
        cpu.load_elf(ElfFile { entry_point: 0, programs: vec![] }).unwrap();
        assert_eq!(cpu.registers[13], 0x8000_0000);
    }

    #[test]
    fn load_elf_writes_each_segment_at_its_virtual_address() {
        let mut cpu = new_cpu();
        let elf = ElfFile {
            entry_point: 0x8000,
            programs: vec![
                ElfProgram { virtual_address: 0x1000, data: vec![0xDE, 0xAD, 0xBE, 0xEF] },
                ElfProgram { virtual_address: 0x4000, data: vec![0x11, 0x22] },
            ],
        };
        cpu.load_elf(elf).unwrap();
        assert_eq!(cpu.memory.read_u8(0x1000).unwrap(), 0xDE);
        assert_eq!(cpu.memory.read_u8(0x1003).unwrap(), 0xEF);
        assert_eq!(cpu.memory.read_u8(0x4000).unwrap(), 0x11);
        assert_eq!(cpu.memory.read_u8(0x4001).unwrap(), 0x22);
    }

    // Cpu::decode_instruction — ARMv7-A A8.3 (cond), A5.7 (unconditional)
    // CondFailed paths are fully self-contained; success paths and the UN path
    // delegate to Instruction::new / Instruction::new_unconditional, which are
    // currently stubbed (todo!()) and will panic when those tests run.

    #[test]
    fn decode_arm_failing_cond_returns_cond_failed() {
        // EQ (cond=0x0) at bits[31:28]; Z=0 → fails. A8.3.
        let cpu = new_cpu();
        let instr = cpu.decode_instruction(0x0000_0000).unwrap();
        assert!(matches!(instr, Instruction::CondFailed));
    }

    #[test]
    fn decode_thumb_it_state_failing_cond_returns_cond_failed() {
        // IT state byte = 0b0000_1000: top 4 bits → cond=EQ, low bits keep IT non-zero.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.cpsr.set_itstate(0b0000_1000);
        // Z=0 so EQ fails.
        let instr = cpu.decode_instruction(0x0000).unwrap();
        assert!(matches!(instr, Instruction::CondFailed));
    }

    #[test]
    fn decode_thumb_no_it_failing_cond_returns_cond_failed() {
        // T1 B<c>: 0xD000 = B EQ #imm. Z=0 so EQ fails.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        let instr = cpu.decode_instruction(0xD000).unwrap();
        assert!(matches!(instr, Instruction::CondFailed));
    }

    #[test]
    fn decode_arm_passing_cond_dispatches_to_decoder() {
        // AL (cond=0xE) always passes → decode_instruction calls Instruction::new.
        let cpu = new_cpu();
        // TODO: assert against the expected Instruction variant once decoder is done
        let _ = cpu.decode_instruction(0xE000_0000);
    }

    #[test]
    fn decode_arm_unconditional_dispatches_to_unconditional_decoder() {
        // cond=0xF (UN) → decode_instruction calls Instruction::new_unconditional. A5.7.
        let cpu = new_cpu();
        // TODO: assert against the expected Instruction variant once decoder is done
        let _ = cpu.decode_instruction(0xF000_0000);
    }

    #[test]
    fn decode_thumb_it_state_passing_cond_dispatches_to_decoder() {
        // IT state with cond=AL (0xE) in top 4 bits → meets_condition true → Instruction::new.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.cpsr.set_itstate(0b1110_1000);
        // TODO: assert against the expected Instruction variant once decoder is done
        let _ = cpu.decode_instruction(0x0000);
    }

    #[test]
    fn decode_thumb_no_it_passing_cond_dispatches_to_decoder() {
        // No IT state, non-branch instruction → thumb_check returns AL → Instruction::new.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        // TODO: assert against the expected Instruction variant once decoder is done
        let _ = cpu.decode_instruction(0x2000); // MOVS r0, #0 (T1)
    }

    // Cpu::execute — ARMv7-A A8.3

    #[test]
    fn execute_cond_failed_leaves_registers_and_psr_unchanged() {
        // A8.3: "If the condition does not pass, the instruction has no effect on registers, memory, or the PSR."
        let mut cpu = new_cpu();
        cpu.write_reg(0, 0xAAAA_AAAA);
        cpu.write_reg(7, 0x5555_5555);
        cpu.cpsr.set_cpsr_bit(cpsr_flags::N, true);
        cpu.cpsr.set_cpsr_bit(cpsr_flags::C, true);
        let cpsr_before = cpu.cpsr.val;
        cpu.execute(&Instruction::CondFailed).unwrap();
        assert_eq!(cpu.registers[0], 0xAAAA_AAAA);
        assert_eq!(cpu.registers[7], 0x5555_5555);
        assert_eq!(cpu.cpsr.val, cpsr_before);
    }

    // Cpu::step — ARMv7-A A2.5 / B1.4.5
    // Each step fetches, decodes, executes; in Thumb mode the IT state advances
    // (B1.4.5 ITAdvance); the PC auto-advances by instr_size unless the executed
    // instruction wrote to it (pc_dirty).

    #[test]
    fn step_arm_after_cond_failed_advances_pc_by_4() {
        // EQ-prefixed MOV r0, #0 with Z=0 → cond fails → execute is a no-op → PC += 4.
        let mut cpu = new_cpu();
        cpu.memory.write_u32_le(0x1000, 0x03A0_0000).unwrap();
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false;
        cpu.step().unwrap();
        assert_eq!(cpu.registers[15], 0x1004);
    }

    #[test]
    fn step_thumb_narrow_after_cond_failed_advances_pc_by_2() {
        // T1 B EQ #imm with Z=0 → cond fails → PC += 2.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.memory.write_u16_le(0x1000, 0xD000).unwrap();
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false;
        cpu.step().unwrap();
        assert_eq!(cpu.registers[15], 0x1002);
    }

    #[test]
    fn step_thumb_wide_after_cond_failed_advances_pc_by_4() {
        // T3 B EQ.W #imm with Z=0 → cond fails → PC += 4.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.memory.write_u16_le(0x1000, 0xF000).unwrap(); // hw1: cond=EQ
        cpu.memory.write_u16_le(0x1002, 0x8000).unwrap(); // hw2: T3 distinguisher
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false;
        cpu.step().unwrap();
        assert_eq!(cpu.registers[15], 0x1004);
    }

    #[test]
    fn step_thumb_advances_it_state_each_iteration() {
        // B1.4.5 ITAdvance: every Thumb instruction inside an IT block advances
        // ITSTATE. Use a CondFailed-causing instruction so we don't depend on a
        // particular Instruction variant being executed.
        let mut cpu = new_cpu();
        cpu.cpsr.set_cpsr_bit(cpsr_flags::T, true);
        cpu.cpsr.set_itstate(0b0000_1001); // top4=EQ, low5=01001 → after advance: 10010
        cpu.memory.write_u16_le(0x1000, 0x0000).unwrap();
        cpu.write_reg(15, 0x1000);
        cpu.pc_dirty = false;
        cpu.step().unwrap();
        assert_eq!(cpu.cpsr.get_itstate(), 0b0001_0010);
    }

    // Cpu::run

    #[test]
    fn run_returns_r0_immediately_when_already_halted() {
        // When halted is true on entry, run does not call step; it returns R0 directly.
        let mut cpu = new_cpu();
        cpu.write_reg(0, 0x42);
        cpu.halted = true;
        assert_eq!(cpu.run().unwrap(), 0x42);
    }
}