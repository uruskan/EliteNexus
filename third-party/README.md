# winit 0.30.13 Windows DPI backport

This copy is from winit 0.30.13 (release commit e9809ef), under its original
Apache-2.0 license. Only the Windows event loop and its utility module are
patched. On Windows 11, trust the window rectangle supplied by the OS. On
earlier or unknown versions, retain the original sizing path. The version
probe passes the structure size to RtlGetVersion.

Upstream fix: https://github.com/rust-windowing/winit/pull/4341
Adaptation for 0.30: https://github.com/rust-windowing/winit/pull/4637
Remove the Cargo patch when a compatible upstream release includes the fix.
