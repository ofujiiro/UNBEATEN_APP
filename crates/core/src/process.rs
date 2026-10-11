//! Is UNBEATABLE running? Under Proton the Windows executable shows up as a
//! process named `UNBEATABLE.exe` (or with that path in its command line).

use std::ffi::{OsStr, OsString};
use std::path::Path;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const GAME_NAMES: [&str; 3] = ["UNBEATABLE.exe", "UNBEATABLE", "UNBEATABLE.x86_64"];

fn file_name_of(arg: &OsStr) -> String {
    let s = arg.to_string_lossy();
    s.rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

pub fn is_game_process(name: &OsStr, cmd: &[OsString], exe: Option<&Path>) -> bool {
    let matches = |n: &str| GAME_NAMES.iter().any(|g| g.eq_ignore_ascii_case(n.trim()));
    matches(&name.to_string_lossy())
        || exe
            .and_then(Path::file_name)
            .is_some_and(|n| matches(&n.to_string_lossy()))
        || cmd.first().is_some_and(|a| matches(&file_name_of(a)))
}

pub struct GameWatch {
    sys: System,
}

impl Default for GameWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl GameWatch {
    pub fn new() -> Self {
        GameWatch { sys: System::new() }
    }

    pub fn is_running(&mut self) -> bool {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
        self.sys
            .processes()
            .values()
            .any(|p| is_game_process(p.name(), p.cmd(), p.exe()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_game_under_proton_and_natively() {
        let none: [OsString; 0] = [];
        assert!(is_game_process(OsStr::new("UNBEATABLE.exe"), &none, None));
        assert!(is_game_process(
            OsStr::new("wine64-preloader"),
            &[OsString::from(
                r"Z:\home\me\.steam\steam\steamapps\common\UNBEATABLE\UNBEATABLE.exe"
            )],
            None
        ));
        assert!(is_game_process(
            OsStr::new("x"),
            &none,
            Some(Path::new("/games/UNBEATABLE.x86_64"))
        ));
        assert!(!is_game_process(
            OsStr::new("UNBEATABLE-launcher"),
            &none,
            None
        ));
        assert!(!is_game_process(
            OsStr::new("steam"),
            &[OsString::from("steam://rungameid/2240620")],
            None
        ));
    }
}
