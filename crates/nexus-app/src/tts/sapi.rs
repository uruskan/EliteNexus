//! SAPI 5, Windows' classic speech engine, through COM. Its voices are Windows' own desktop voices and the ones other
//! programs install for it (IVONA, CereProc, …), which Windows speech (WinRT) doesn't offer.
//! Everything here runs on the speech thread, which [`Com::init`] has joined to COM.

use super::VoiceInfo;
use windows::Win32::Media::Speech::{
    ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES, SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SPRS_DONE, SPVOICESTATUS, SpObjectTokenCategory, SpVoice,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows::core::{PCWSTR, PWSTR};

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
            Ok(Sapi { voice, default, tokens })
        }
    }

    pub fn voices(&self) -> Vec<VoiceInfo> {
        self.tokens.iter().map(|(v, _)| v.clone()).collect()
    }

    /// The voice with this id (empty: the default one). False when it isn't installed.
    pub fn set_voice(&self, id: &str) -> bool {
        let token = if id.is_empty() { self.default.as_ref() } else { self.tokens.iter().find(|(v, _)| v.id == id).map(|(_, t)| t) };
        // SAFETY: as in open.
        token.is_some_and(|t| unsafe { self.voice.SetVoice(t) }.is_ok())
    }

    pub fn set_rate(&self, rate: f32) {
        // SAFETY: as in open.
        let _ = unsafe { self.voice.SetRate(sapi_rate(rate)) };
    }

    pub fn set_volume(&self, volume: f32) {
        // SAFETY: as in open.
        let _ = unsafe { self.voice.SetVolume(sapi_volume(volume)) };
    }

    /// Speaks one line and waits for it (so back-to-back alerts queue rather than interrupt), up to 2 minutes like
    /// the speech programs on Linux: a voice stuck longer is cut off, so the lines after it are still heard.
    pub fn speak(&self, text: &str) -> Result<(), String> {
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        // Plain text: a name with "<" in it can't be read as SAPI's XML.
        let flags = (SPF_ASYNC.0 | SPF_IS_NOT_XML.0) as u32;
        // SAFETY: as in open; `wide` outlives the speaking: it's waited for, or purged, before returning.
        unsafe {
            self.voice.Speak(PCWSTR(wide.as_ptr()), flags, None).map_err(|e| e.to_string())?;
            let spoken = self.wait();
            if !matches!(spoken, Ok(true)) {
                // SAPI may still be reading `wide`. Without SPF_ASYNC the purge returns once the voice has stopped.
                let _ = self.voice.Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None);
            }
            match spoken {
                Ok(true) => Ok(()),
                Ok(false) => Err("it didn't finish within 2 minutes".into()),
                Err(e) => Err(e.to_string()),
            }
        }
    }

    /// Waits up to 2 minutes for the line: true once it has been spoken.
    unsafe fn wait(&self) -> windows::core::Result<bool> {
        // SAFETY: as in open.
        unsafe {
            // Running out of time isn't an error here: the voice's state says whether it finished.
            self.voice.WaitUntilDone(120_000)?;
            let (mut state, mut bookmark) = (SPVOICESTATUS::default(), PWSTR::null());
            let status = self.voice.GetStatus(&mut state, &mut bookmark);
            if !bookmark.is_null() {
                CoTaskMemFree(Some(bookmark.0 as *const _));
            }
            status?;
            state.hrLastResult.ok()?;
            Ok(state.dwRunningState & SPRS_DONE.0 as u32 != 0)
        }
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

    /// Speaks each installed SAPI voice's name out loud: `cargo test -p nexus-app sapi_speaks -- --ignored`.
    #[test]
    #[ignore]
    fn sapi_speaks_with_each_voice() {
        let _com = Com::init();
        let sapi = Sapi::open().expect("SAPI");
        let voices = sapi.voices();
        assert!(!voices.is_empty(), "no SAPI voices installed");
        for v in &voices {
            assert!(sapi.set_voice(&v.id), "{}", v.name);
            sapi.speak(&v.name).expect("spoken");
        }
        assert!(sapi.set_voice(""));
        assert!(!sapi.set_voice("no such voice"));
        sapi.speak("Back to the default voice. <b>Not</b> XML.").expect("spoken");
    }
}
