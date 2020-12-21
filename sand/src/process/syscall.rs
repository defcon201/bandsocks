use crate::{
    abi,
    process::{loader::Loader, task::StoppedTask},
    protocol::{
        abi::Syscall, Errno, FileStat, FromTask, LogLevel, LogMessage, SysFd, ToTask, VFile, VPtr,
        VString,
    },
    remote::{
        file::{RemoteFd, TempRemoteFd},
        scratchpad::Scratchpad,
        trampoline::Trampoline,
    },
};
use sc::nr;

#[derive(Debug)]
pub struct SyscallEmulator<'q, 's, 't> {
    stopped_task: &'t mut StoppedTask<'q, 's>,
    call: Syscall,
}

impl<'q, 's, 't> SyscallEmulator<'q, 's, 't> {
    pub fn new(stopped_task: &'t mut StoppedTask<'q, 's>) -> Self {
        let call = Syscall::from_regs(stopped_task.regs);
        SyscallEmulator { stopped_task, call }
    }

    async fn return_file(&mut self, _vfile: VFile, sys_fd: SysFd) -> isize {
        let mut tr = Trampoline::new(self.stopped_task);
        let result = match Scratchpad::new(&mut tr).await {
            Err(err) => Err(err),
            Ok(mut scratchpad) => {
                let result = RemoteFd::from_local(&mut scratchpad, &sys_fd).await;
                scratchpad.free().await.expect("leaking scratchpad page");
                result
            }
        };
        match result {
            Ok(RemoteFd(fd)) => fd as isize,
            Err(err) => self.return_errno(err).await,
        }
    }

    async fn return_errno(&mut self, err: Errno) -> isize {
        if err.0 >= 0 {
            panic!("invalid {:?}", err);
        }
        err.0 as isize
    }

    async fn return_result(&mut self, result: Result<(), Errno>) -> isize {
        match result {
            Ok(()) => 0,
            Err(err) => self.return_errno(err).await,
        }
    }

    async fn return_file_result(&mut self, result: Result<(VFile, SysFd), Errno>) -> isize {
        match result {
            Ok((vfile, sys_fd)) => self.return_file(vfile, sys_fd).await,
            Err(err) => self.return_errno(err).await,
        }
    }

    async fn return_stat_result(
        &mut self,
        _out_ptr: VPtr,
        _result: Result<FileStat, Errno>,
    ) -> isize {
        // to do
        -1
    }

    async fn return_vptr_result(&mut self, result: Result<VPtr, Errno>) -> isize {
        match result {
            Ok(ptr) => ptr.0 as isize,
            Err(err) => self.return_errno(err).await,
        }
    }

    async fn return_size_result(&mut self, result: Result<usize, Errno>) -> isize {
        match result {
            Ok(s) => s as isize,
            Err(err) => self.return_errno(err).await,
        }
    }

    pub async fn dispatch(&mut self) {
        let args = self.call.args;
        let arg_i32 = |idx| args[idx] as i32;
        let arg_usize = |idx| args[idx] as usize;
        let arg_ptr = |idx| VPtr(arg_usize(idx));
        let arg_string = |idx| VString(arg_ptr(idx));
        let mut log_level = LogLevel::Debug;

        let result = match self.call.nr as usize {
            nr::BRK => {
                let ptr = arg_ptr(0);
                let result = do_brk(self.stopped_task, ptr).await;
                self.return_vptr_result(result).await
            }

            nr::EXECVE => {
                let filename = arg_string(0);
                let argv = arg_ptr(1);
                let envp = arg_ptr(2);
                let result = Loader::execve(self.stopped_task, filename, argv, envp).await;
                self.return_result(result).await
            }

            nr::GETPID => self.stopped_task.task.task_data.vpid.0 as isize,

            nr::GETPPID => {
                log_level = LogLevel::Warn;
                1
            }

            nr::GETUID => {
                log_level = LogLevel::Warn;
                0
            }

            nr::GETGID => {
                log_level = LogLevel::Warn;
                0
            }

            nr::GETEUID => {
                log_level = LogLevel::Warn;
                0
            }

            nr::GETEGID => {
                log_level = LogLevel::Warn;
                0
            }

            nr::GETPGRP => {
                log_level = LogLevel::Warn;
                0
            }

            nr::SETPGID => {
                log_level = LogLevel::Warn;
                0
            }

            nr::GETPGID => {
                log_level = LogLevel::Warn;
                0
            }

            nr::UNAME => {
                let result = do_uname(self.stopped_task, arg_ptr(0)).await;
                self.return_result(result).await
            }

            nr::SYSINFO => {
                log_level = LogLevel::Warn;
                0
            }

            nr::SET_TID_ADDRESS => {
                log_level = LogLevel::Warn;
                0
            }

            nr::IOCTL => {
                log_level = LogLevel::Warn;
                let _fd = arg_i32(0);
                let _cmd = arg_i32(1);
                let _arg = arg_usize(2);
                0
            }

            nr::STAT => {
                log_level = LogLevel::Warn;
                let result = ipc_call!(
                    self.stopped_task.task,
                    FromTask::FileStat {
                        file: None,
                        path: Some(arg_string(0)),
                        nofollow: false
                    },
                    ToTask::FileStatReply(result),
                    result
                );
                self.return_stat_result(arg_ptr(1), result).await
            }

            nr::FSTAT => {
                log_level = LogLevel::Warn;
                0
            }

            nr::LSTAT => {
                log_level = LogLevel::Warn;
                let result = ipc_call!(
                    self.stopped_task.task,
                    FromTask::FileStat {
                        file: None,
                        path: Some(arg_string(0)),
                        nofollow: true
                    },
                    ToTask::FileStatReply(result),
                    result
                );
                self.return_stat_result(arg_ptr(1), result).await
            }

            nr::NEWFSTATAT => {
                log_level = LogLevel::Warn;
                let flags = arg_i32(3);
                let fd = arg_i32(0);
                if fd != abi::AT_FDCWD {
                    unimplemented!();
                }
                let result = ipc_call!(
                    self.stopped_task.task,
                    FromTask::FileStat {
                        file: None,
                        path: Some(arg_string(1)),
                        nofollow: (flags & abi::AT_SYMLINK_NOFOLLOW) != 0
                    },
                    ToTask::FileStatReply(result),
                    result
                );
                self.return_stat_result(arg_ptr(2), result).await
            }

            nr::ACCESS => {
                log_level = LogLevel::Warn;
                let result = ipc_call!(
                    self.stopped_task.task,
                    FromTask::FileAccess {
                        dir: None,
                        path: arg_string(0),
                        mode: arg_i32(1),
                    },
                    ToTask::Reply(result),
                    result
                );
                self.return_result(result).await
            }

            nr::GETCWD => {
                log_level = LogLevel::Warn;
                let result = ipc_call!(
                    self.stopped_task.task,
                    FromTask::GetWorkingDir(arg_string(0), arg_usize(1)),
                    ToTask::SizeReply(result),
                    result
                );
                self.return_size_result(result).await
            }

            nr::CHDIR => {
                log_level = LogLevel::Warn;
                let result = ipc_call!(
                    self.stopped_task.task,
                    FromTask::ChangeWorkingDir(arg_string(0)),
                    ToTask::Reply(result),
                    result
                );
                self.return_result(result).await
            }

            nr::OPEN => {
                let result = ipc_call!(
                    self.stopped_task.task,
                    FromTask::FileOpen {
                        dir: None,
                        path: arg_string(0),
                        flags: arg_i32(1),
                        mode: arg_i32(2),
                    },
                    ToTask::FileReply(result),
                    result
                );
                self.return_file_result(result).await
            }

            nr::OPENAT if arg_i32(0) == abi::AT_FDCWD => {
                let fd = arg_i32(0);
                if fd != abi::AT_FDCWD {
                    log_level = LogLevel::Error;
                }
                let result = ipc_call!(
                    self.stopped_task.task,
                    FromTask::FileOpen {
                        dir: None,
                        path: arg_string(1),
                        flags: arg_i32(2),
                        mode: arg_i32(3),
                    },
                    ToTask::FileReply(result),
                    result
                );
                self.return_file_result(result).await
            }

            _ => {
                log_level = LogLevel::Error;
                self.return_result(Err(Errno(-abi::ENOSYS))).await
            }
        };
        self.call.ret = result;
        Syscall::ret_to_regs(self.call.ret, self.stopped_task.regs);

        if self.stopped_task.task.log_enabled(log_level) {
            self.stopped_task
                .task
                .log(log_level, LogMessage::Emulated(self.call.clone()))
        }
    }
}

async fn do_uname<'q, 's, 't>(
    stopped_task: &'t mut StoppedTask<'q, 's>,
    dest: VPtr,
) -> Result<(), Errno> {
    let mut tr = Trampoline::new(stopped_task);
    let mut pad = Scratchpad::new(&mut tr).await?;
    let temp = TempRemoteFd::new(&mut pad).await?;
    let result = Ok(());
    let result = result.and(
        temp.mem_write_bytes_exact(
            &mut pad,
            dest.add(offset_of!(abi::UtsName, sysname)),
            b"Linux\0",
        )
        .await,
    );
    let result = result.and(
        temp.mem_write_bytes_exact(
            &mut pad,
            dest.add(offset_of!(abi::UtsName, nodename)),
            b"host\0",
        )
        .await,
    );
    let result = result.and(
        temp.mem_write_bytes_exact(
            &mut pad,
            dest.add(offset_of!(abi::UtsName, release)),
            b"4.0.0-bandsocks\0",
        )
        .await,
    );
    let result = result.and(
        temp.mem_write_bytes_exact(
            &mut pad,
            dest.add(offset_of!(abi::UtsName, version)),
            b"#1 SMP\0",
        )
        .await,
    );
    let result = result.and(
        temp.mem_write_bytes_exact(
            &mut pad,
            dest.add(offset_of!(abi::UtsName, machine)),
            abi::PLATFORM_NAME_BYTES,
        )
        .await,
    );
    pad.free().await?;
    temp.free(&mut tr).await?;
    result
}

/// brk() is emulated using mmap because we can't change the host kernel's per
/// process brk pointer from our loader without extra privileges.
async fn do_brk<'q, 's, 't>(
    stopped_task: &'t mut StoppedTask<'q, 's>,
    new_brk: VPtr,
) -> Result<VPtr, Errno> {
    if new_brk.0 != 0 {
        let old_brk = stopped_task.task.task_data.mm.brk;
        let brk_start = stopped_task.task.task_data.mm.brk_start;
        let old_brk_page = VPtr(abi::page_round_up(brk_start.max(old_brk).0));
        let new_brk_page = VPtr(abi::page_round_up(brk_start.max(new_brk).0));
        if new_brk_page != old_brk_page {
            let mut tr = Trampoline::new(stopped_task);
            if new_brk_page == brk_start {
                tr.munmap(brk_start, old_brk_page.0 - brk_start.0).await?;
            } else if old_brk_page == brk_start {
                tr.mmap_anonymous_noreplace(
                    brk_start,
                    new_brk_page.0 - brk_start.0,
                    abi::PROT_READ | abi::PROT_WRITE,
                )
                .await?;
            } else {
                tr.mremap(
                    brk_start,
                    old_brk_page.0 - brk_start.0,
                    new_brk_page.0 - brk_start.0,
                )
                .await?;
            }
        }
        stopped_task.task.task_data.mm.brk = brk_start.max(new_brk);
    }
    Ok(stopped_task.task.task_data.mm.brk)
}
