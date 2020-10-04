// This code may not be used for any purpose. Be gay, do crime.

use crate::bpf::*;
use crate::abi::*;
use sc::{syscall, nr};

// This file has two policies; the "tracer" policy is applied very early, and covers this
// process for its entire lifetime. The "loader" policy is applied during stage 2, and it
// applies additional ruless which the sandbox contents use but not the tracer.
//
// For comparison, the container we might be running in likely has a policy like this one:
// https://github.com/moby/moby/blob/master/profiles/seccomp/default.json

fn base_rules_for_all_policies() -> ProgramBuffer {
    let mut p = ProgramBuffer::new();

    // Keep syscall in the accumulator generally
    p.inst(load(offset_of!(SeccompData, nr)));

    // Fully allowed in all modes
    // to do: none of this has been audited yet. this will generally be all syscalls
    //        that deal with existing fds or with memory, but nothing that deals with
    //        pids and nothing that has a pathname in it.
    p.if_any_eq(&[
        nr::READ,
        nr::WRITE,
        nr::PREAD64,
        nr::PWRITE64,
        nr::READV,
        nr::WRITEV,
        nr::SENDMSG,
        nr::RECVMSG,        
        nr::CLOSE,
        nr::FCNTL,
        nr::EXIT_GROUP,        
        nr::EXIT,
        nr::RT_SIGRETURN,
        nr::FORK,
        nr::BRK,
        nr::COPY_FILE_RANGE,
        nr::SENDFILE,
    ], &[
        ret(SECCOMP_RET_ALLOW)
    ]);
    p
}
        
pub fn policy_for_tracer() {
    let mut p = base_rules_for_all_policies();
    
    // these are emulated inside the sandbox, but the tracer is allowed to use them
    // to do: none of this has been audited yet
    p.if_any_eq(&[
        nr::ARCH_PRCTL,
        nr::PRCTL,
        nr::WAITID,
        nr::PTRACE,
        nr::GETPID,

        // need this to get to the next stage
        // xxx: drop this privilege as soon as we initialize the tracer
        nr::EXECVE,

        // xxx: can't allow this, use a different attach mechanism?
        nr::KILL,
        
    ], &[
        ret(SECCOMP_RET_ALLOW)
    ]);

    // There is no tracer yet, but we want to allow tracing later.
    // With no tracer attached this blocks the syscall with ENOSYS.
    p.inst(ret(SECCOMP_RET_TRACE));

    activate(&p);
}

pub fn policy_for_loader() {
    let mut p = base_rules_for_all_policies();

    // Specific deny list, of calls we don't even want to try and trace or emulate
    p.if_any_eq(&[
        nr::PTRACE,
    ], &[
        ret(SECCOMP_RET_KILL_PROCESS)
    ]);
    
    // Emulate supported syscalls, rely on the tracer to log and panic on others
    p.inst(ret(SECCOMP_RET_TRACE));

    activate(&p);
}

fn activate(program_buffer: &ProgramBuffer) {
    let prog = program_buffer.to_filter_prog();
    let ptr = (&prog) as *const SockFilterProg as usize;
    let result = unsafe {
        syscall!(PRCTL, PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        syscall!(PRCTL, PR_SET_SECCOMP, SECCOMP_MODE_FILTER, ptr, 0, 0) as isize
    };
    if result != 0 {
        panic!("seccomp setup error ({})", result);
    }
}
