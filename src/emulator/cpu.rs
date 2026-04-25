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
    NV = 0xF, // Never (Reserved/Special cases)
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
            15 => Condition::NV,
            _ => unreachable!(),
        }
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
    pub registers: [u32; 16],
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
        if (self.cpsr.get_cpsr_bit(cpsr_flags::T))
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
            Condition::NV => false,
        }
    }

    pub fn decode_instruction(&self, data: u32) -> Result<Instruction, Fault> {
        let cond_bits = (data >> 28) as u8;
        let cond = Condition::from_u4(cond_bits);
        if !self.meets_condition(cond) {
            return Ok(Instruction::CondFailed);
        }
        Instruction::new(data)
    }

    pub fn execute(&mut self, instr: &Instruction) -> Result<(), Fault> {
        todo!()
    }

    pub fn step(&mut self) -> Result<(), Fault> {
        let (raw, instr_size) = self.fetch()?;
        let instr = self.decode_instruction(raw)?;
        self.execute(&instr)?;

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
}