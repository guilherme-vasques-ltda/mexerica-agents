pub(super) struct Pins<'a> {
    helper: &'a str,
    hub: &'a str,
}

impl<'a> Pins<'a> {
    pub(super) fn parse(input: &'a str) -> Option<Self> {
        let (helper, hub) = input.strip_suffix('\n')?.split_once('\n')?;
        (valid_hash(helper) && valid_hash(hub) && helper != hub).then_some(Self { helper, hub })
    }

    pub(super) fn helper_requirement(&self) -> String {
        format!("cdhash H\"{}\"", self.helper)
    }
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn display_hash(output: &str) -> Option<&str> {
    let mut candidates = output.lines().filter_map(|line| line.strip_prefix("CDHash="));
    let hash = candidates.next()?;
    (candidates.next().is_none() && valid_hash(hash)).then_some(hash)
}

#[cfg(target_os = "macos")]
pub(super) mod macos {
    use super::{Pins, display_hash};
    use anyhow::{Context, Result, ensure};
    use serde_json::Value;
    use std::{
        ffi::{CStr, CString, c_char, c_uint, c_void},
        fs::File,
        io::Read,
        path::Path,
        process::Command,
    };

    unsafe extern "C" {
        fn mexerica_screen_xpc_send(
            service: *const c_char,
            requirement: *const c_char,
            json: *const c_char,
        ) -> *mut c_void;
        fn mexerica_screen_xpc_finish(handle: *mut c_void, seconds: c_uint) -> *mut c_char;
    }

    pub(in crate::screen) struct Pending(Option<std::ptr::NonNull<c_void>>);

    // The native object owns its state on a serial queue, not on the sending thread.
    unsafe impl Send for Pending {}

    impl Pending {
        pub(in crate::screen) fn wait(mut self, seconds: u32) -> Result<Value> {
            let handle = self.0.take().context("screen request was already closed")?;
            let reply = unsafe { mexerica_screen_xpc_finish(handle.as_ptr(), seconds) };
            ensure!(!reply.is_null(), "no authenticated reply from the screen helper");
            let decoded = unsafe { serde_json::from_slice::<Value>(CStr::from_ptr(reply).to_bytes()) };
            unsafe { libc::free(reply.cast()) };
            decoded.context("invalid screen helper response")
        }
    }
    impl Drop for Pending {
        fn drop(&mut self) {
            if let Some(handle) = self.0.take() {
                let reply = unsafe { mexerica_screen_xpc_finish(handle.as_ptr(), 0) };
                unsafe { libc::free(reply.cast()) };
            }
        }
    }

    pub(in crate::screen) struct Xpc {
        requirement: CString,
    }

    impl Xpc {
        pub(in crate::screen) fn packaged() -> Result<Self> {
            let exe = std::env::current_exe()?;
            ensure!(exe.file_name().and_then(|name| name.to_str()) == Some("donnabot-host"), "not the packaged Hub");
            let resources = exe.parent().context("no package resources")?;
            let contents = resources.parent().context("no package contents")?;
            let bundle = contents.parent().context("no app bundle")?;
            ensure!(
                resources.ends_with("Resources")
                    && contents.ends_with("Contents")
                    && bundle.extension().is_some_and(|ext| ext == "app"),
                "not a packaged Hub"
            );
            let helper = contents.join("Library/LoginItems/DonnabotDevScreen.app");
            let binary = helper.join("Contents/MacOS/DonnabotDevScreen");
            let verify = |path: &Path, flags: &[&str]| {
                let status = Command::new("/usr/bin/codesign").env_clear().args(flags).arg(path).output()?;
                ensure!(status.status.success(), "invalid packaged screen signature");
                Ok(())
            };
            verify(bundle, &["--verify", "--strict"])?;
            verify(&helper, &["--verify", "--deep", "--strict"])?;
            let mut pin = String::new();
            File::open(resources.join("screen-identity.cdhashes"))?.take(83).read_to_string(&mut pin)?;
            let pins = Pins::parse(&pin).context("invalid packaged screen pins")?;
            let read_matches = |path: &Path, pinned: &str| -> Result<bool> {
                let info = Command::new("/usr/bin/codesign")
                    .env_clear()
                    .args(["--display", "--verbose=4"])
                    .arg(path)
                    .output()?;
                Ok(info.status.success() && display_hash(std::str::from_utf8(&info.stderr)?) == Some(pinned))
            };
            ensure!(
                read_matches(&binary, pins.helper)? && read_matches(&exe, pins.hub)?,
                "screen package pin mismatch"
            );
            Ok(Self { requirement: CString::new(pins.helper_requirement())? })
        }

        pub(in crate::screen) fn send(&self, request: &Value) -> Result<Pending> {
            let request = CString::new(request.to_string())?;
            ensure!(request.as_bytes().len() < 32_768, "screen request is too large");
            // The native bridge copies the request before this call returns.
            let handle = unsafe {
                mexerica_screen_xpc_send(
                    c"app.donnabot.dev.screen.xpc".as_ptr(),
                    self.requirement.as_ptr(),
                    request.as_ptr(),
                )
            };
            Ok(Pending(Some(std::ptr::NonNull::new(handle).context("no authenticated screen channel")?)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELPER: &str = "1234567890abcdef1234567890abcdef12345678";
    const HUB: &str = "abcdef1234567890abcdef1234567890abcdef12";

    #[test]
    fn pins_identify_the_two_distinct_executables() {
        let text = format!("{HELPER}\n{HUB}\n");
        let pins = Pins::parse(&text).unwrap();
        assert_eq!(pins.helper, HELPER);
        assert_eq!(pins.hub, HUB);
        assert_eq!(pins.helper_requirement(), format!("cdhash H\"{HELPER}\""));
    }

    #[test]
    fn pins_reject_missing_or_ambiguous_identities() {
        for invalid in [
            String::new(),
            format!("{HELPER}\n"),
            format!("{HELPER}\n{HELPER}\n"),
            format!("{HELPER}\n{HUB}\n{HUB}\n"),
            format!("{HELPER}\n{}\n", HUB.to_ascii_uppercase()),
            format!("{HELPER} or true\n{HUB}\n"),
        ] {
            assert!(Pins::parse(&invalid).is_none());
        }
    }

    #[test]
    fn codesign_must_report_exactly_one_valid_hash() {
        let display = format!("Identifier=example.invalid\nCDHash={HELPER}\n");
        assert_eq!(display_hash(&display), Some(HELPER));
        assert_eq!(display_hash(&format!("{display}CDHash=invalid\n")), None);
        assert_eq!(display_hash("Identifier=example.invalid\n"), None);
    }
}
