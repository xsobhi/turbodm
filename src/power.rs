//! "When done": open the file, sleep or shut down (the UIs count down 30 seconds first).

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Power {
    #[default]
    Nothing = 0,
    Sleep = 1,
    Shutdown = 2,
}

/// Chosen in a progress window's "Options on completion" tab.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnDone {
    pub open_file: bool,
    pub power: Power,
}

pub const COUNTDOWN_SECONDS: u32 = 30;

impl Power {
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Power::Sleep,
            2 => Power::Shutdown,
            _ => Power::Nothing,
        }
    }

    pub fn run(self) {
        // logind lets the active local session do this without a password
        #[cfg(not(windows))]
        let command = ("systemctl", vec![if self == Power::Sleep { "suspend" } else { "poweroff" }]);
        #[cfg(windows)]
        let command = match self {
            Power::Sleep => ("rundll32.exe", vec!["powrprof.dll,SetSuspendState", "0,1,0"]),
            _ => ("shutdown.exe", vec!["/s", "/t", "0"]),
        };
        if let Err(err) = std::process::Command::new(command.0).args(&command.1).spawn() {
            eprintln!("turbodm: {}: {err}", command.0);
        }
    }
}
