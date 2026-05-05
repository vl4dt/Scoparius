use winreg::enums::*;
use winreg::RegKey;
use chrono::{TimeZone, Utc};

fn rot13(s: &str) -> String {
    s.chars().map(|c| {
        match c {
            'a'..='m' | 'A'..='M' => ((c as u8) + 13) as char,
            'n'..='z' | 'N'..='Z' => ((c as u8) - 13) as char,
            _ => c,
        }
    }).collect()
}

fn filetime_to_utc(ft: u64) -> Option<chrono::DateTime<Utc>> {
    if ft == 0 { return None; }
    // 116444736000000000 is diff between 1601 and 1970 in 100ns
    let t = ft.saturating_sub(116444736000000000);
    let secs = t / 10_000_000;
    let nsecs = (t % 10_000_000) * 100;
    Utc.timestamp_opt(secs as i64, nsecs as u32).single()
}

fn main() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(ua) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist") {
        for guid in ua.enum_keys().filter_map(|k| k.ok()) {
            if let Ok(count) = ua.open_subkey(format!(r"{}\Count", guid)) {
                for (name, val) in count.enum_values().filter_map(|v| v.ok()) {
                    let decoded_name = rot13(&name);
                    let data: Vec<u8> = val.bytes;
                    if data.len() >= 68 {
                        let mut ft_bytes = [0u8; 8];
                        ft_bytes.copy_from_slice(&data[60..68]);
                        let ft = u64::from_le_bytes(ft_bytes);
                        if let Some(dt) = filetime_to_utc(ft) {
                            println!("{} -> {}", decoded_name, dt);
                        }
                    }
                }
            }
        }
    }
}
