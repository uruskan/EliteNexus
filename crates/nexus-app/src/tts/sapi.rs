//! SAPI 5, Windows' classic speech engine, through COM. Its voices are Windows' own desktop voices and the ones other
//! programs install for it (IVONA, CereProc, …), which Windows speech (WinRT) doesn't offer.
//! Everything here runs on the speech thread, which [`Com::init`] has joined to COM.

use super::VoiceInfo;
use std::time::{Duration, Instant};
use windows::Win32::Media::Speech::{
    ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES, SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SPVOICESTATUS, SpObjectTokenCategory, SpVoice,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows::core::{Interface, PCWSTR, PWSTR};

/// A line still being spoken after this long is cut off, as the speech programs on Linux are, so the lines after it
/// are still heard: only a stuck voice takes that long.
const LINE_LIMIT: Duration = Duration::from_secs(120);

/// COM on this thread, from `init` until dropped (on the same thread).
pub struct Com(bool);

impl Com {
    /// The multithreaded apartment: SAPI's voice works in it, and WinRT, the other engine, joins it by itself anyway.
    pub fn init() -> Com {
        // SAFETY: undone once by Drop, on this thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if let Err(e) = hr.ok() {
            tracing::warn!("COM didn't start for speech: {e}");
        }
        Com(hr.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: pairs the successful CoInitializeEx above.
            unsafe { CoUninitialize() }
        }
    }
}

pub struct Sapi {
    voice: ISpVoice,
    /// The voice it started with (Control Panel › Speech's choice), for going back to it.
    default: Option<ISpObjectToken>,
    /// The installed voices, with the tokens `SetVoice` takes.
    tokens: Vec<(VoiceInfo, ISpObjectToken)>,
    /// The line being spoken: its text, which SAPI reads until the line is done or stopped, and when it started.
    line: Option<(Vec<u16>, Instant)>,
}

impl Sapi {
    pub fn open() -> Result<Sapi, String> {
        // SAFETY: COM is joined on this thread (Com::init) and the objects never leave it.
        unsafe {
            let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
            let default = voice.GetVoice().ok();
            let tokens = installed_voices().unwrap_or_else(|e| {
                tracing::warn!("SAPI's voices couldn't be listed: {e}");
                Vec::new()
            });
            Ok(Sapi { voice, default, tokens, line: None })
        }
    }

    pub fn voices(&self) -> Vec<VoiceInfo> {
        self.tokens.iter().map(|(v, _)| v.clone()).collect()
    }

    /// Lists the installed voices again: one installed since shows up.
    pub fn reload(&mut self) {
        // SAFETY: as in open.
        match unsafe { installed_voices() } {
            Ok(tokens) => self.tokens = tokens,
            Err(e) => tracing::warn!("SAPI's voices couldn't be listed: {e}"),
        }
    }

    /// The voice with this id (empty: the default one), from the next line on. False when it isn't installed; one
    /// installed since the list was read is looked for again first.
    pub fn set_voice(&mut self, id: &str) -> bool {
        if !id.is_empty() && !self.tokens.iter().any(|(v, _)| v.id == id) {
            self.reload();
        }
        let token = if id.is_empty() { self.default.as_ref() } else { self.tokens.iter().find(|(v, _)| v.id == id).map(|(_, t)| t) };
        // SAFETY: as in open.
        token.is_some_and(|t| unsafe { self.voice.SetVoice(t) }.is_ok())
    }

    /// The speed: SAPI applies it to the line being spoken too.
    pub fn set_rate(&self, rate: f32) {
        // SAFETY: as in open.
        let _ = unsafe { self.voice.SetRate(sapi_rate(rate)) };
    }

    /// The volume: SAPI applies it to the line being spoken too.
    pub fn set_volume(&self, volume: f32) {
        // SAFETY: as in open.
        let _ = unsafe { self.voice.SetVolume(sapi_volume(volume)) };
    }

    /// Starts speaking one line and returns: [`Sapi::wait`] says when it's done. One line at a time, so back-to-back
    /// alerts queue rather than interrupt, while a new rate or volume still reaches the line being spoken.
    pub fn speak(&mut self, text: &str) -> Result<(), String> {
        self.stop();
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        // Plain text: a name with "<" in it can't be read as SAPI's XML.
        let flags = (SPF_ASYNC.0 | SPF_IS_NOT_XML.0) as u32;
        // SAFETY: as in open; `wide` is kept in `line` until the voice is done with it or has been stopped.
        unsafe { self.voice.Speak(PCWSTR(wide.as_ptr()), flags, None) }.map_err(|e| e.to_string())?;
        self.line = Some((wide, Instant::now()));
        Ok(())
    }

    /// A line is being spoken.
    pub fn speaking(&self) -> bool {
        self.line.is_some()
    }

    /// Waits up to `up_to` for the line being spoken. Once it has ended, how it went: spoken, or why not (an error, or
    /// still speaking after 2 minutes, when it's cut off). None while it's still being spoken, or with no line.
    pub fn wait(&mut self, up_to: Duration) -> Option<Result<(), String>> {
        let started = self.line.as_ref()?.1;
        let ms = u32::try_from(up_to.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: as in open.
        let result = match unsafe { self.done_within(ms) } {
            Ok(true) => Ok(()),
            Ok(false) if started.elapsed() < LINE_LIMIT => return None,
            Ok(false) => Err("it didn't finish within 2 minutes".to_string()),
            Err(e) => Err(e.to_string()),
        };
        if result.is_err() {
            self.stop();
        }
        self.line = None;
        Some(result)
    }

    /// Stops the line being spoken. SAPI no longer reads its text when this returns: without SPF_ASYNC the purge
    /// returns once the voice has stopped.
    fn stop(&mut self) {
        if self.line.is_some() {
            // SAFETY: as in open.
            let _ = unsafe { self.voice.Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None) };
            self.line = None;
        }
    }

    /// Waits up to `ms` for the line: true once it has been spoken, false while it still is.
    unsafe fn done_within(&self, ms: u32) -> windows::core::Result<bool> {
        // SAFETY: as in open. The method's Result can't tell S_FALSE (the time ran out) from S_OK (done); the call
        // through the interface's table can.
        unsafe {
            let hr = (Interface::vtable(&self.voice).WaitUntilDone)(Interface::as_raw(&self.voice), ms);
            hr.ok()?;
            if hr.0 != 0 {
                return Ok(false);
            }
            // Done: whether the engine reported an error on the way.
            let (mut state, mut bookmark) = (SPVOICESTATUS::default(), PWSTR::null());
            let status = self.voice.GetStatus(&mut state, &mut bookmark);
            if !bookmark.is_null() {
                CoTaskMemFree(Some(bookmark.0 as *const _));
            }
            status?;
            state.hrLastResult.ok()?;
            Ok(true)
        }
    }
}

impl Drop for Sapi {
    /// A line still being spoken is stopped first: SAPI may still be reading its text.
    fn drop(&mut self) {
        self.stop();
    }
}

/// Every voice installed for SAPI, named by its description (as SAPI's own SpGetDescription reads it).
unsafe fn installed_voices() -> windows::core::Result<Vec<(VoiceInfo, ISpObjectToken)>> {
    // SAFETY: the caller's (COM joined on this thread).
    unsafe {
        let category: ISpObjectTokenCategory = CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
        category.SetId(SPCAT_VOICES, false)?;
        let list = category.EnumTokens(PCWSTR::null(), PCWSTR::null())?;
        let mut count = 0;
        list.GetCount(&mut count)?;
        let mut voices = Vec::new();
        for i in 0..count {
            let token = list.Item(i)?;
            let Some(id) = take_string(token.GetId()) else { continue };
            // A token's default value is its description, e.g. "Microsoft Zira Desktop - English (United States)".
            let name = take_string(token.GetStringValue(PCWSTR::null())).unwrap_or_else(|| id.clone());
            voices.push((VoiceInfo { id, name }, token));
        }
        Ok(voices)
    }
}

/// A string SAPI allocated: copied, then freed.
unsafe fn take_string(s: windows::core::Result<PWSTR>) -> Option<String> {
    let s = s.ok()?;
    // SAFETY: SAPI hands over a NUL-terminated string allocated with CoTaskMemAlloc, freed once here.
    unsafe {
        let text = s.to_string().ok();
        CoTaskMemFree(Some(s.0 as *const _));
        text
    }
}

/// The speed setting (1.0 = normal) as SAPI's rate, -10 to 10 with 0 normal. Each step is a ratio: 10 is about three
/// times the normal speed and -10 about a third, so 0.5× and 2× come out at -6 and 6.
pub fn sapi_rate(rate: f32) -> i32 {
    (10.0 * rate.max(0.01).ln() / 3f32.ln()).clamp(-10.0, 10.0).round() as i32
}

/// The volume setting (0 to 1) as SAPI's, 0 to 100.
pub fn sapi_volume(volume: f32) -> u16 {
    (volume.clamp(0.0, 1.0) * 100.0).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sapi_ranges() {
        assert_eq!((sapi_rate(1.0), sapi_rate(2.0), sapi_rate(0.5), sapi_rate(3.0), sapi_rate(10.0), sapi_rate(0.1)), (0, 6, -6, 10, 10, -10));
        assert_eq!((sapi_volume(1.0), sapi_volume(0.9), sapi_volume(0.5), sapi_volume(2.0), sapi_volume(-1.0)), (100, 90, 50, 100, 0));
    }

    /// Speaks a line and waits for it, as the speech thread does.
    fn say(sapi: &mut Sapi, text: &str) -> Result<(), String> {
        sapi.speak(text)?;
        loop {
            if let Some(result) = sapi.wait(Duration::from_millis(50)) {
                return result;
            }
        }
    }

    /// Speaks each installed SAPI voice's name out loud: `cargo test -p nexus-app sapi_speaks -- --ignored`.
    #[test]
    #[ignore]
    fn sapi_speaks_with_each_voice() {
        let _com = Com::init();
        let mut sapi = Sapi::open().expect("SAPI");
        let voices = sapi.voices();
        assert!(!voices.is_empty(), "no SAPI voices installed");
        for v in &voices {
            assert!(sapi.set_voice(&v.id), "{}", v.name);
            say(&mut sapi, &v.name).expect("spoken");
        }
        assert!(sapi.set_voice(""));
        assert!(!sapi.set_voice("no such voice"));
        say(&mut sapi, "Back to the default voice. <b>Not</b> XML.").expect("spoken");
    }

    /// The speech thread's way of speaking, silently (volume 0): a line is still being spoken at its first look, takes a
    /// new speed and volume meanwhile, and says how it went once it ends; dropping the engine mid-line stops it.
    /// `cargo test -p nexus-app sapi_silent -- --ignored`
    #[test]
    #[ignore]
    fn sapi_silent_lines_end_while_settings_change() {
        let _com = Com::init();
        let mut sapi = Sapi::open().expect("SAPI");
        sapi.set_volume(0.0);
        sapi.speak("A long line, spoken silently, so that it is still being spoken when it is looked at for the first time.").expect("speaking");
        assert!(sapi.speaking());
        assert!(sapi.wait(Duration::from_millis(50)).is_none(), "still speaking");
        sapi.set_rate(2.0);
        sapi.set_volume(0.0);
        let started = Instant::now();
        let result = loop {
            if let Some(r) = sapi.wait(Duration::from_millis(50)) {
                break r;
            }
            assert!(started.elapsed() < Duration::from_secs(30), "the line ends");
        };
        assert_eq!(result, Ok(()));
        assert!(!sapi.speaking());
        assert!(sapi.wait(Duration::from_millis(10)).is_none(), "no line");
        sapi.speak("Stopped halfway, silently.").expect("speaking");
        drop(sapi);
    }

    /// Lowers the volume and raises the speed halfway through a line: both are heard at once, before it ends.
    /// `cargo test -p nexus-app sapi_changes -- --ignored`
    #[test]
    #[ignore]
    fn sapi_changes_reach_the_line_being_spoken() {
        let _com = Com::init();
        let mut sapi = Sapi::open().expect("SAPI");
        sapi.speak("This line starts at normal speed and full volume, then gets faster and quieter before it ends, while it is still being spoken.").expect("speaking");
        assert!(sapi.wait(Duration::from_millis(2500)).is_none(), "still speaking");
        sapi.set_rate(1.8);
        sapi.set_volume(0.3);
        let result = loop {
            if let Some(r) = sapi.wait(Duration::from_millis(50)) {
                break r;
            }
        };
        assert_eq!(result, Ok(()));
        assert!(!sapi.speaking());
    }
}
