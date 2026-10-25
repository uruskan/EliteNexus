//! Alerts in XSOverlay, a VR overlay for SteamVR: the important ones go to its notification port on this computer as
//! JSON over UDP (127.0.0.1:42069, its legacy UDP API). Off unless switched on in Settings › Overlay; nothing is sent
//! anywhere else.

use nexus_model::{Alert, Priority};
use std::net::UdpSocket;

/// XSOverlay's notification port.
const PORT: &str = "127.0.0.1:42069";

#[derive(Default)]
pub struct XsOverlay {
    socket: Option<UdpSocket>,
}

impl XsOverlay {
    /// Shows `a` in XSOverlay when it's important enough for the island. Nothing waits for an answer: with XSOverlay
    /// closed the message is simply dropped.
    pub fn send(&mut self, a: &Alert) {
        if !super::worth_showing(a) {
            return;
        }
        if self.socket.is_none() {
            self.socket = UdpSocket::bind("127.0.0.1:0").inspect_err(|e| tracing::debug!("XSOverlay socket: {e}")).ok();
        }
        if let Some(s) = &self.socket
            && let Err(e) = s.send_to(payload(a).as_bytes(), PORT)
        {
            tracing::debug!("XSOverlay: {e}");
        }
    }
}

/// The notification: the alert's title and detail for four seconds, silent (Elite Nexus speaks alerts itself).
pub fn payload(a: &Alert) -> String {
    let height = if a.detail.is_empty() { 110.0 } else { 175.0 };
    let icon = if a.priority == Priority::Critical { "error" } else { "default" };
    serde_json::json!({
        "messageType": 1,
        "index": 0,
        "timeout": 4.0,
        "height": height,
        "opacity": 1.0,
        "volume": 0.0,
        "audioPath": "",
        "title": a.title,
        "content": a.detail,
        "useBase64Icon": false,
        "icon": icon,
        "sourceApp": "Elite Nexus",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_journal::Timestamp;
    use nexus_model::AlertKind;

    #[test]
    fn a_notification_xsoverlay_reads() {
        let a = Alert::new(Timestamp(0), AlertKind::Combat, Priority::Critical, "Hull critical").detail("Hull at 20%");
        let v: serde_json::Value = serde_json::from_str(&payload(&a)).expect("JSON");
        assert_eq!(v["messageType"], 1);
        assert_eq!((v["title"].as_str(), v["content"].as_str()), (Some("Hull critical"), Some("Hull at 20%")));
        assert_eq!((v["icon"].as_str(), v["audioPath"].as_str(), v["sourceApp"].as_str()), (Some("error"), Some(""), Some("Elite Nexus")));
    }
}
