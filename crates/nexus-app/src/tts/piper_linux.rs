//! Play Piper PCM through the PulseAudio protocol (also provided by PipeWire on Linux desktops).

use std::borrow::Cow;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::null;

#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

type OpenStream = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_int,
    *const c_char,
    *const c_char,
    *const SampleSpec,
    *const c_void,
    *const c_void,
    *mut c_int,
) -> *mut c_void;
type WriteStream = unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut c_int) -> c_int;
type DrainStream = unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int;
type FreeStream = unsafe extern "C" fn(*mut c_void);
type ErrorText = unsafe extern "C" fn(c_int) -> *const c_char;

pub(super) fn play(wav: &[u8], volume: f32) -> Result<(), String> {
    const PA_SAMPLE_S16LE: c_int = 3;
    const PA_STREAM_PLAYBACK: c_int = 1;

    // Both the cache and the worker validate this fixed PCM WAV layout before calling play.
    let spec = SampleSpec {
        format: PA_SAMPLE_S16LE,
        rate: u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
        channels: 1,
    };
    let pcm = &wav[44..];
    let pcm = if volume < 1.0 {
        Cow::Owned(
            pcm.as_chunks::<2>()
                .0
                .iter()
                .flat_map(|&bytes| {
                    ((f32::from(i16::from_le_bytes(bytes)) * volume) as i16).to_le_bytes()
                })
                .collect(),
        )
    } else {
        Cow::Borrowed(pcm)
    };
    // SAFETY: the system library's stable libpulse-simple ABI matches the declarations.
    // All symbols and the stream are used before unloading the library.
    unsafe {
        let lib = libloading::Library::new("libpulse-simple.so.0").map_err(|e| {
            format!(
                "Piper audio needs PulseAudio or PipeWire's PulseAudio compatibility layer ({e})"
            )
        })?;
        let open = lib
            .get::<OpenStream>(b"pa_simple_new")
            .map_err(|e| e.to_string())?;
        let write = lib
            .get::<WriteStream>(b"pa_simple_write")
            .map_err(|e| e.to_string())?;
        let drain = lib
            .get::<DrainStream>(b"pa_simple_drain")
            .map_err(|e| e.to_string())?;
        let free = lib
            .get::<FreeStream>(b"pa_simple_free")
            .map_err(|e| e.to_string())?;
        let error = lib
            .get::<ErrorText>(b"pa_strerror")
            .map_err(|e| e.to_string())?;
        let mut code = 0;
        let stream = open(
            null(),
            c"Elite Nexus".as_ptr(),
            PA_STREAM_PLAYBACK,
            null(),
            c"Piper alerts".as_ptr(),
            &spec,
            null(),
            null(),
            &mut code,
        );
        let played = if stream.is_null() {
            false
        } else {
            let played = write(stream, pcm.as_ptr(), pcm.len(), &mut code) == 0
                && drain(stream, &mut code) == 0;
            free(stream);
            played
        };
        if played {
            Ok(())
        } else {
            let text = error(code);
            let text = if text.is_null() {
                format!("error {code}")
            } else {
                CStr::from_ptr(text).to_string_lossy().into_owned()
            };
            Err(format!(
                "Piper couldn't play audio through the system sound server ({text})"
            ))
        }
    }
}
