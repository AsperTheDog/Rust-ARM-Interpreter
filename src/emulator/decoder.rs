use crate::emulator::cpu::cpsr_flags;
use crate::emulator::Fault;

pub enum Instruction {
    CondFailed,

}

impl Instruction {
    pub fn new(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    pub fn is_wide_op(op: u32) -> bool {
        let op= op & cpsr_flags::MODE_MASK;
        matches!(op, 0b11101 | 0b11110 | 0b11111)
    }
}