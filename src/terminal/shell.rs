//! The shells ctermin can start.

use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellKind {
    #[default]
    PowerShell,
    PowerShell7,
    Cmd,
    GitBash,
    Wsl,
}

const ALL_SHELLS: [ShellKind; 5] = [
    ShellKind::PowerShell,
    ShellKind::PowerShell7,
    ShellKind::Cmd,
    ShellKind::GitBash,
    ShellKind::Wsl,
];

fn in_program_files(relative: &str) -> Option<PathBuf> {
    let root = std::env::var_os("ProgramFiles")?;
    Some(Path::new(&root).join(relative))
}

fn in_system32(file: &str) -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")?;
    Some(Path::new(&root).join("System32").join(file))
}

impl ShellKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::PowerShell => "PowerShell",
            Self::PowerShell7 => "PowerShell 7",
            Self::Cmd => "Command Prompt",
            Self::GitBash => "Git Bash",
            Self::Wsl => "WSL",
        }
    }

    /// Full path for shells that are optional. `None` for shells always found through `PATH`.
    fn installed_path(self) -> Option<PathBuf> {
        match self {
            Self::PowerShell | Self::Cmd => None,
            Self::PowerShell7 => in_program_files(r"PowerShell\7\pwsh.exe"),
            Self::GitBash => in_program_files(r"Git\bin\bash.exe"),
            Self::Wsl => in_system32("wsl.exe"),
        }
    }

    fn is_available(self) -> bool {
        match self {
            Self::PowerShell | Self::Cmd => true,
            _ => self.installed_path().is_some_and(|path| path.exists()),
        }
    }

    /// Installed shells, with `default` first.
    pub fn available(default: Self) -> Vec<Self> {
        let others = ALL_SHELLS
            .into_iter()
            .filter(|shell| *shell != default && shell.is_available());
        std::iter::once(default).chain(others).collect()
    }

    pub fn command(self) -> CommandBuilder {
        let mut command = match self {
            Self::PowerShell => CommandBuilder::new("powershell.exe"),
            Self::Cmd => CommandBuilder::new("cmd.exe"),
            Self::PowerShell7 | Self::GitBash | Self::Wsl => {
                let program = self.installed_path().unwrap_or_default();
                CommandBuilder::new(program)
            }
        };
        match self {
            Self::PowerShell | Self::PowerShell7 => command.arg("-NoLogo"),
            Self::GitBash => {
                command.args(["--login", "-i"]);
                // Keeps the login shell in the folder ctermin starts it in.
                command.env("CHERE_INVOKING", "1");
            }
            Self::Cmd | Self::Wsl => {}
        }
        if let Some(home) = std::env::var_os("USERPROFILE") {
            command.cwd(home);
        }
        command
    }

    /// Describes the shell to the AI.
    pub fn environment(self) -> &'static str {
        match self {
            Self::PowerShell => "Windows PowerShell on Windows",
            Self::PowerShell7 => "PowerShell 7 on Windows",
            Self::Cmd => "Command Prompt (cmd.exe) on Windows",
            Self::GitBash => "Git Bash on Windows",
            Self::Wsl => "A Linux shell inside WSL on Windows",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_comes_first_without_duplicates() {
        let shells = ShellKind::available(ShellKind::Cmd);
        assert_eq!(shells[0], ShellKind::Cmd);
        assert_eq!(
            shells
                .iter()
                .filter(|shell| **shell == ShellKind::Cmd)
                .count(),
            1
        );
        assert!(shells.contains(&ShellKind::PowerShell));
    }

    #[test]
    fn powershell_starts_without_a_logo() {
        let command = ShellKind::PowerShell.command();
        let args: Vec<_> = command
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert_eq!(args, ["powershell.exe", "-NoLogo"]);
    }

    #[test]
    fn git_bash_is_a_login_shell() {
        let command = ShellKind::GitBash.command();
        assert!(command.get_argv()[0]
            .to_string_lossy()
            .ends_with("bash.exe"));
        assert_eq!(command.get_argv()[1], "--login");
    }

    #[test]
    fn every_shell_has_a_name_and_description() {
        for shell in ALL_SHELLS {
            assert!(!shell.label().is_empty());
            assert!(shell.environment().contains("Windows"));
        }
    }
}
