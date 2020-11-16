use crate::{
    abi,
    abi::UserRegs,
    process::loader::{FileHeader, Loader},
    protocol::{Errno, VPtr},
};
use goblin::elf64::{header, header::Header, program_header, program_header::ProgramHeader};

fn elf64_header(fh: &FileHeader) -> Header {
    *plain::from_bytes(&fh.bytes).unwrap()
}

fn elf64_program_header(loader: &Loader, ehdr: &Header, idx: u16) -> Result<ProgramHeader, Errno> {
    let mut header = Default::default();
    let bytes = unsafe { plain::as_mut_bytes(&mut header) };
    loader.read(
        ehdr.e_phoff as usize + ehdr.e_phentsize as usize * idx as usize,
        bytes,
    )?;
    Ok(header)
}

pub fn detect(fh: &FileHeader) -> bool {
    let ehdr = elf64_header(fh);
    &ehdr.e_ident[..header::SELFMAG] == header::ELFMAG
        && ehdr.e_ident[header::EI_CLASS] == header::ELFCLASS64
        && ehdr.e_ident[header::EI_DATA] == header::ELFDATA2LSB
        && ehdr.e_ident[header::EI_VERSION] == header::EV_CURRENT
}

async fn replace_userspace<'q, 's, 't>(loader: &mut Loader<'q, 's, 't>, sp: u64, ip: u64) {
    let prev_regs = loader.userspace_regs().clone();
    loader.userspace_regs().clone_from(&UserRegs {
        sp,
        ip,
        cs: prev_regs.cs,
        ss: prev_regs.ss,
        ds: prev_regs.ds,
        es: prev_regs.es,
        fs: prev_regs.fs,
        gs: prev_regs.gs,
        flags: prev_regs.flags,
        ..Default::default()
    });

    loader.unmap_all_userspace_mem().await;
}

fn phdr_prot(phdr: &ProgramHeader) -> isize {
    let mut prot = 0;
    if 0 != (phdr.p_flags & program_header::PF_R) {
        prot |= abi::PROT_READ
    }
    if 0 != (phdr.p_flags & program_header::PF_W) {
        prot |= abi::PROT_WRITE
    }
    if 0 != (phdr.p_flags & program_header::PF_X) {
        prot |= abi::PROT_EXEC
    }
    prot
}

pub async fn load<'q, 's, 't>(mut loader: Loader<'q, 's, 't>) -> Result<(), Errno> {
    let ehdr = elf64_header(loader.file_header());
    println!("ELF64 {:?}", ehdr);

    // todo: lets have a stack
    loader
        .map_anonymous(VPtr(0x10000), 0x10000, abi::PROT_READ | abi::PROT_WRITE)
        .await?;
    let sp = 0x1fff0;
    replace_userspace(&mut loader, sp, ehdr.e_entry).await;

    for idx in 0..ehdr.e_phnum {
        let phdr = elf64_program_header(&loader, &ehdr, idx)?;
        if phdr.p_type == program_header::PT_LOAD
            && abi::page_offset(phdr.p_offset as usize) == abi::page_offset(phdr.p_vaddr as usize)
        {
            let prot = phdr_prot(&phdr);
            let page_alignment = abi::page_offset(phdr.p_vaddr as usize);

            if phdr.p_memsz > phdr.p_filesz {
                loader
                    .map_anonymous(
                        VPtr(phdr.p_vaddr as usize - page_alignment),
                        abi::page_round_up(phdr.p_memsz as usize + page_alignment),
                        prot,
                    )
                    .await?;
            }

            if phdr.p_filesz > 0 {
                loader
                    .map_file(
                        VPtr(phdr.p_vaddr as usize - page_alignment),
                        abi::page_round_up(phdr.p_filesz as usize + page_alignment),
                        phdr.p_offset as usize - page_alignment,
                        prot,
                    )
                    .await?;
            }

            println!(
                "{}/{}, {:x?}",
                idx,
                ehdr.e_phnum,
                (
                    phdr.p_type,
                    phdr.p_flags,
                    phdr.p_offset,
                    phdr.p_vaddr,
                    phdr.p_paddr,
                    phdr.p_filesz,
                    phdr.p_memsz,
                    phdr.p_align
                ),
            );
        }
    }

    loader.debug_loop().await;
    Ok(())
}
