pub mod cpu;
pub mod memory;
pub mod decoder;

#[derive(Debug)]
pub enum Fault {
    /// The PC is not 4-byte aligned
    Alignment { address: u32 },

    /// The decoder encountered a bit pattern it doesn't recognize
    UndefinedInstruction { raw_bits: u32 },

    /// Accessing a page that isn't mapped in the Page Table
    Translation { address: u32, is_write: bool },

    /// Writing to a read-only page
    Permission { address: u32 },

    /// A Software Interrupt (SVC) - handled like a fault in the loop
    SoftwareInterrupt { immediate: u32 },
    
    /// Unknown - Created mainly just for errors that don't have a clear fault
    Unknown {},
}