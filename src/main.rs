mod elf;
mod utils;
mod emulator;

use clap::Parser;
use utils::args::ArgData;
use elf::reader::ElfFile;
use emulator::memory::paged_mem::PagedMemory;
use emulator::cpu::Cpu;

fn main() {
    let arg: ArgData = ArgData::parse();

    let elf = ElfFile::new(&arg.file).expect("failed to parse ELF file");

    let mem = PagedMemory::new();
    let mut cpu: Cpu<PagedMemory> = Cpu::new(mem);
    cpu.load_elf(elf).expect("failed to load ELF file onto memory");

    let res =cpu.run().expect("failed to run CPU");
    std::process::exit(res as i32);
}
