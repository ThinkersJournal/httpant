// SPDX-License-Identifier: MIT OR Apache-2.0
//! Events a TLS listener reports for the audit trail (WebHostAnt need 23, for AuditAnt). They say what happened and to which name; they never
//! carry key material, certificate bytes, or the text of an error from the certificate source (a source's message is its own business and could
//! hold anything).

use std::collections::HashMap;
use std::sync::Mutex;

/// Longest server name an event carries, in bytes (a DNS name is at most 253).
const MAX_NAME_LEN: usize = 253;

/// At most this many names are remembered to notice a certificate changing; a client that invents names cannot make the memory grow without
/// bound. Once full, names not yet remembered are served as before but their changes are not reported.
const MAX_TRACKED_NAMES: usize = 1024;

/// Why a handshake was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefusalReason {
    /// The [`CertSource`](crate::CertSource) does not know the name the client asked for.
    UnknownName,
    /// The source failed (its error is not repeated here).
    SourceFailed,
    /// The client did not finish the handshake in time.
    Timeout,
    /// The client broke the protocol, or sent nothing a TLS server can use.
    Protocol,
}

/// Something that happened on a TLS listener. Names are client-supplied text: they are cut to 253 bytes and anything that is not printable
/// ASCII is replaced by `?`, so a sink may write them into a log line as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TlsEvent {
    /// A handshake was refused.
    HandshakeRefused {
        /// The server name the client sent, if it got as far as sending one.
        server_name: Option<String>,
        /// Why.
        reason: RefusalReason,
    },
    /// The certificate served for a name is not the one served for it before (a renewal, or a replacement). Not sent for the first
    /// certificate a name is served.
    CertificateChanged {
        /// The server name the client sent, `None` for a client that sent none.
        server_name: Option<String>,
        /// The serial number of the certificate now served, in hexadecimal.
        serial: String,
    },
}

/// Receives [`TlsEvent`]s. Called on the connection's task, so it must not block: queue the event and return.
pub trait TlsAudit: Send + Sync + 'static {
    /// Something happened.
    fn event(&self, event: &TlsEvent);
}

/// Cut a client-supplied name to something safe to log.
pub(crate) fn clean_name(name: &str) -> String {
    name.bytes()
        .take(MAX_NAME_LEN)
        .map(|b| {
            if b.is_ascii_graphic() {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect()
}

/// Remembers the certificate last served for each name, to notice a change.
#[derive(Debug, Default)]
pub(crate) struct Served(Mutex<HashMap<Option<String>, Vec<u8>>>);

impl Served {
    /// Record that `leaf` (DER) is being served for `name`. `true` when this name had a different certificate before.
    pub(crate) fn changed(&self, name: &Option<String>, leaf: &[u8]) -> bool {
        let mut seen = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match seen.get_mut(name) {
            Some(old) if old.as_slice() == leaf => false,
            Some(old) => {
                *old = leaf.to_vec();
                true
            }
            None => {
                if seen.len() < MAX_TRACKED_NAMES {
                    seen.insert(name.clone(), leaf.to_vec());
                }
                false
            }
        }
    }
}

/// The serial number of a DER certificate, in hexadecimal; empty if it does not parse.
pub(crate) fn serial_hex(leaf: &[u8]) -> String {
    x509_parser::parse_x509_certificate(leaf)
        .map(|(_, c)| c.raw_serial_as_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_in_events_are_cut_and_made_printable() {
        assert_eq!(clean_name("example.com"), "example.com");
        assert_eq!(clean_name("a\r\nb c\u{1b}[0m\u{e9}"), "a??b?c?[0m??");
        assert_eq!(clean_name(&"a".repeat(1000)).len(), MAX_NAME_LEN);
    }

    #[test]
    fn a_change_is_reported_once_and_never_for_the_first_certificate() {
        let s = Served::default();
        let name = Some("a.test".to_owned());
        assert!(!s.changed(&name, b"one"), "first certificate for a name");
        assert!(!s.changed(&name, b"one"), "same certificate");
        assert!(s.changed(&name, b"two"), "replaced");
        assert!(!s.changed(&name, b"two"), "the change was already reported");
        assert!(!s.changed(&None, b"x"), "another name has its own history");
        assert!(s.changed(&None, b"y"));
    }

    #[test]
    fn invented_names_cannot_grow_the_memory_without_bound() {
        let s = Served::default();
        for i in 0..(MAX_TRACKED_NAMES + 500) {
            let _ = s.changed(&Some(format!("n{i}.test")), b"c");
        }
        assert_eq!(s.0.lock().unwrap().len(), MAX_TRACKED_NAMES);
    }
}
