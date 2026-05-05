use std::io;
use std::process::{Child, Command, Output, Stdio};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn apply_no_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(target_os = "windows"))]
    let _ = command;
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn output_no_window(command: &mut Command) -> io::Result<Output> {
    apply_no_window(command);
    command.output()
}

pub(crate) fn spawn_no_window(command: &mut Command) -> io::Result<Child> {
    apply_no_window(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}
