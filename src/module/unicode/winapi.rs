use std::collections::HashMap;
use std::ffi::CString;
use std::mem::size_of;
use std::os::raw::{c_int, c_uchar, c_void};
use std::process::Command;
use std::ptr::null;
use std::{thread, time};

use crate::module::unicode::UnicodeOutput;

use winapi::ctypes::wchar_t;
use winapi::um::{winnls, winnt, winuser};

const KEY_DELAY_US: u64 = 60000;

pub struct DisplayConnection {
    charmap: HashMap<char, u32>,
    held: Vec<char>,
}

impl Default for DisplayConnection {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayConnection {
    pub fn new() -> DisplayConnection {
        unsafe {
            let charmap = HashMap::new();
            let held = Vec::new();
            DisplayConnection { charmap, held }
        }
    }

    pub fn press_key(&self, c: wchar_t, state: bool) {
        let flags = if state {
            winuser::KEYEVENTF_UNICODE // Defaults to down
        } else {
            winuser::KEYEVENTF_UNICODE | winuser::KEYEVENTF_KEYUP
        };

        let mut input = unsafe {
            let mut i: winuser::INPUT_u = std::mem::zeroed();
            let mut ki = i.ki_mut();
            ki.wScan = c;
            ki.dwFlags = flags;

            winuser::INPUT {
                type_: winuser::INPUT_KEYBOARD,
                u: i,
            }
        };
        unsafe {
            winuser::SendInput(1, &mut input, size_of::<winuser::INPUT>() as i32);
        }
    }
}

impl Drop for DisplayConnection {
    fn drop(&mut self) {
        info!("Releasing all unicode keys");
        self.set_held("");
    }
}

impl UnicodeOutput for DisplayConnection {
    fn get_layout(&self) -> String {
        let result = Command::new("powershell")
            .args(&["-Command", "Get-WinUserLanguageList"])
            .output()
            .expect("Failed to exec");
        let output = String::from_utf8_lossy(&result.stdout);
        let mut map = output
            .lines()
            .filter(|l| l.contains(':'))
            .map(|l| l.split(':'))
            .map(|mut kv| (kv.next().unwrap().trim(), kv.next().unwrap().trim()));
        let layout = map
            .find(|(k, _): &(&str, &str)| *k == "LanguageTag")
            .map(|(_, v)| v)
            .unwrap_or("");
        layout.to_string()
    }

    fn set_layout(&self, layout: &str) {
        Command::new("powershell")
            .args(&[
                "-Command",
                &format!("Set-WinUserLanguageList -Force '{}'", &layout),
            ])
            .output();
    }

    fn type_string(&mut self, string: &str) {
        for c in string.encode_utf16() {
            self.press_key(c, true);
        }
    }

    fn press_symbol(&mut self, c: char, press: bool) {
        let mut buff = [0; 2];
        c.encode_utf16(&mut buff);
        for k in buff.iter() {
            self.press_key(*k, press);
        }

        if press {
            self.held.push(c);
        } else {
            self.held
                .iter()
                .position(|&x| x == c)
                .map(|e| self.held.remove(e));
        }
    }

    fn get_held(&mut self) -> Vec<char> {
        self.held.clone()
    }

    fn set_held(&mut self, string: &str) {
        let s: Vec<char> = string.chars().collect();
        for c in &self.held.clone() {
            if !s.contains(c) {
                self.press_symbol(*c, false);
            }
        }
        for c in &s {
            self.press_symbol(*c, true);
        }
    }
}
