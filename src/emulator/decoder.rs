use crate::emulator::decoder::Instruction::YIELD;
use crate::emulator::Fault;
use crate::utils::{get_bit, get_bits};

#[derive(Debug, Clone, Copy)]
pub enum Operand2 {
    Immediate(u32),

    ShiftedRegister {
        rm: u8,
        shift_type: ShiftType,
        amount: ShiftAmount,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ShiftType {
    Lsl, // Logical Shift Left
    Lsr, // Logical Shift Right
    Asr, // Arithmetic Shift Right
    Ror, // Rotate Right
    Rrx, // Rotate Right Extended (ROR #0 special case)
}

#[derive(Debug, Clone, Copy)]
pub enum ShiftAmount {
    Immediate(u8),
    Register(u8),
}

pub enum Instruction {
    CondFailed,
    NOP,
    YIELD,
    WFE,
    WFI,
    SEV,
    CSBD,
    DBG { option: u8 },
    MOVW { rd: u8, imm: u16 },
    MOVT { rd: u8, imm: u16 }
}

impl Instruction {
    pub fn new_arm(data: u32) -> Result<Self, Fault> {
        let family = get_bits(data, 27, 25);   //(data >> 25) & 0x7;

        match family {
            0b000 => Instruction::decode_data_processing(data),
            0b001 => Instruction::decode_data_processing_imm(data),
            0b010 => Instruction::decode_load_store(data),
            0b011 => Instruction::decode_load_store_imm(data),
            0b100 => Instruction::decode_block_transfer(data),
            0b101 => Instruction::decode_branch(data),
            0b110 => Instruction::decode_coproc_load_store(data),
            0b111 => Instruction::decode_svc_or_coproc(data),
            _ => Err(Fault::UndefinedInstruction{ raw_bits: data })
        }
    }

    pub fn new_thumb(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    pub fn new_unconditional(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    fn decode_data_processing(data: u32) -> Result<Self, Fault> {
        let op1 = get_bits(data, 24, 20);

        let op2_first = get_bit(data, 7);
        let op2_last = get_bit(data, 4);
        if op2_first && op2_last {

        }
        else {

        }
    }

    fn decode_data_processing_imm(data: u32) -> Result<Self, Fault> {
        let op1 = get_bits(data, 24, 20);
        match op1 {
            0b10000 => {
                let imm = ((get_bits(data, 19, 16) << 12) | get_bits(data, 11, 0)) as u16;
                Ok(Instruction::MOVW { rd: get_bits(data, 15, 12) as u8, imm })
            },
            0b10100 => {
                let imm = ((get_bits(data, 19, 16) << 12) | get_bits(data, 11, 0)) as u16;
                Ok(Instruction::MOVT { rd: get_bits(data, 15, 12) as u8, imm })
            },
            0b10010 | 0b10110 => {
                let in_op = get_bit(data, 22);
                let in_op1 = get_bits(data, 19, 16);

                match in_op1 {
                    0b0000 => {
                        let in_op2 = get_bits(data, 7, 0);
                        match in_op2 {
                            0b00000000 => {
                                Ok(Instruction::NOP)
                            },
                            0b00000001 => {
                                Ok(Instruction::YIELD)
                            },
                            0b00000010 => {
                                Ok(Instruction::WFE)
                            },
                            0b00000011 => {
                                Ok(Instruction::WFI)
                            },
                            0b00000100 => {
                                Ok(Instruction::SEV)
                            },
                            0b00010100 => {
                                Ok(Instruction::CSBD)
                            },
                            _ => {
                                let check = get_bits(data, 7, 4);
                                if check != 0xF {
                                    return Err(Fault::UndefinedInstruction{ raw_bits: data });
                                }
                                let option = get_bits(data, 3, 0) as u8;
                                Ok(Instruction::DBG{option})
                            }
                        }
                    },
                    n if (n & 0b11) == 0 => {
                        let mask = get_bits(data, 19, 18);
                        let imm12 = get_bits(data, 11, 0);
                        
                    },
                    _ => {

                    },
                }
            }
            _ => {

            }
        }
    }

    fn decode_load_store(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    fn decode_load_store_imm(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    fn decode_block_transfer(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    fn decode_branch(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    fn decode_coproc_load_store(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    fn decode_svc_or_coproc(data: u32) -> Result<Self, Fault> {
        todo!()
    }

    fn decode_operand2(data: u32, is_immediate: bool) -> Operand2 {
        if is_immediate {
            let imm = data & 0xFF;
            let rotate = (data >> 8) & 0xF;
            Operand2::Immediate(imm.rotate_right(rotate * 2))
        } else {
            let rm = (data & 0xF) as u8;
            let shift_type = match (data >> 5) & 0x3 {
                0b00 => ShiftType::Lsl,
                0b01 => ShiftType::Lsr,
                0b10 => ShiftType::Asr,
                0b11 => ShiftType::Ror,
                _ => unreachable!(),
            };

            let amount = if (data >> 4) & 1 == 0 {
                ShiftAmount::Immediate(((data >> 7) & 0x1F) as u8)
            } else {
                ShiftAmount::Register(((data >> 8) & 0xF) as u8)
            };

            Operand2::ShiftedRegister { rm, shift_type, amount }
        }
    }

    pub fn is_wide_op(op: u32) -> bool {
        let prefix = (op >> 11) & 0x1F;
        matches!(prefix, 0b11101 | 0b11110 | 0b11111)
    }
}