use anyhow::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io::stdout;
use std::path::Path;
use std::process::{Command, ExitStatus};

#[allow(dead_code)]
/// Suspend the TUI, run an external command, then restore the TUI.
///
/// Steps:
/// 1. Leave alternate screen
/// 2. Disable raw mode
/// 3. Run the command (blocking, inheriting stdin/stdout/stderr)
/// 4. Re-enable raw mode
/// 5. Re-enter alternate screen
/// 6. Force terminal redraw (via the returned `needs_redraw` signal)
///
/// Returns the command's exit status.
pub fn run_external_command(program: &str, args: &[&str], working_dir: &Path) -> Result<ExitStatus> {
    run_external_command_with(&mut RealTerminal, program, args, working_dir)
}

/// Testable version that accepts a [`TerminalControl`] implementation.
pub fn run_external_command_with<T: TerminalControl>(
    terminal: &mut T,
    program: &str,
    args: &[&str],
    working_dir: &Path,
) -> Result<ExitStatus> {
    terminal.leave_alternate_screen()?;
    terminal.disable_raw_mode()?;

    let status = Command::new(program)
        .args(args)
        .current_dir(working_dir)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    // Always attempt to restore the terminal, even if the command failed to run.
    terminal.enable_raw_mode()?;
    terminal.enter_alternate_screen()?;

    Ok(status?)
}

/// Abstraction over terminal operations for testability.
pub trait TerminalControl {
    fn leave_alternate_screen(&mut self) -> Result<()>;
    fn disable_raw_mode(&mut self) -> Result<()>;
    fn enable_raw_mode(&mut self) -> Result<()>;
    fn enter_alternate_screen(&mut self) -> Result<()>;
}

/// Real implementation that talks to the actual terminal.
struct RealTerminal;

impl TerminalControl for RealTerminal {
    fn leave_alternate_screen(&mut self) -> Result<()> {
        execute!(stdout(), LeaveAlternateScreen)?;
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> Result<()> {
        disable_raw_mode()?;
        Ok(())
    }

    fn enable_raw_mode(&mut self) -> Result<()> {
        enable_raw_mode()?;
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> Result<()> {
        execute!(stdout(), EnterAlternateScreen)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records the sequence of terminal operations for verification.
    struct MockTerminal {
        ops: RefCell<Vec<&'static str>>,
    }

    impl MockTerminal {
        fn new() -> Self {
            Self {
                ops: RefCell::new(Vec::new()),
            }
        }

        fn ops(&self) -> Vec<&'static str> {
            self.ops.borrow().clone()
        }
    }

    impl TerminalControl for MockTerminal {
        fn leave_alternate_screen(&mut self) -> Result<()> {
            self.ops.borrow_mut().push("leave_alternate_screen");
            Ok(())
        }

        fn disable_raw_mode(&mut self) -> Result<()> {
            self.ops.borrow_mut().push("disable_raw_mode");
            Ok(())
        }

        fn enable_raw_mode(&mut self) -> Result<()> {
            self.ops.borrow_mut().push("enable_raw_mode");
            Ok(())
        }

        fn enter_alternate_screen(&mut self) -> Result<()> {
            self.ops.borrow_mut().push("enter_alternate_screen");
            Ok(())
        }
    }

    #[test]
    fn terminal_operations_in_correct_order() {
        let mut mock = MockTerminal::new();
        let dir = tempfile::tempdir().unwrap();

        let status =
            run_external_command_with(&mut mock, "echo", &["hello"], dir.path()).unwrap();

        assert!(status.success());
        assert_eq!(
            mock.ops(),
            vec![
                "leave_alternate_screen",
                "disable_raw_mode",
                "enable_raw_mode",
                "enter_alternate_screen",
            ]
        );
    }

    #[test]
    fn returns_exit_status_of_command() {
        let mut mock = MockTerminal::new();
        let dir = tempfile::tempdir().unwrap();

        let status =
            run_external_command_with(&mut mock, "sh", &["-c", "exit 42"], dir.path()).unwrap();

        assert!(!status.success());
        assert_eq!(status.code(), Some(42));
    }

    #[test]
    fn runs_in_specified_working_directory() {
        let mut mock = MockTerminal::new();
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker.txt");

        let status = run_external_command_with(
            &mut mock,
            "sh",
            &["-c", "echo ok > marker.txt"],
            dir.path(),
        )
        .unwrap();

        assert!(status.success());
        assert!(marker.exists());
    }

    #[test]
    fn terminal_restored_even_when_command_fails() {
        let mut mock = MockTerminal::new();
        let dir = tempfile::tempdir().unwrap();

        let status =
            run_external_command_with(&mut mock, "sh", &["-c", "exit 1"], dir.path()).unwrap();

        assert!(!status.success());
        // Terminal should still be restored
        assert_eq!(
            mock.ops(),
            vec![
                "leave_alternate_screen",
                "disable_raw_mode",
                "enable_raw_mode",
                "enter_alternate_screen",
            ]
        );
    }

    #[test]
    fn error_when_program_not_found() {
        let mut mock = MockTerminal::new();
        let dir = tempfile::tempdir().unwrap();

        let result = run_external_command_with(
            &mut mock,
            "nonexistent_program_that_should_not_exist",
            &[],
            dir.path(),
        );

        assert!(result.is_err());
        // Terminal should still be restored even when the command can't be spawned
        assert_eq!(
            mock.ops(),
            vec![
                "leave_alternate_screen",
                "disable_raw_mode",
                "enable_raw_mode",
                "enter_alternate_screen",
            ]
        );
    }
}
