// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTPAnt adapter conformance suite (design `docs/HTTPANT-DESIGN.md`, section 6).
//!
//! - [`CASES`]: nineteen cases, each run over a **real socket** against a [`Subject`] (an adapter), speaking through [`raw`], the suite's own
//!   minimal HTTP/1.1 client. The client is deliberately not HTTPAnt, hyper or any adapter, so a defect cannot hide on both ends of a test.
//! - Every case lists the known-bad [`Mutant`]s it must fail on. [`check_case`] demands both halves: the reference passes **and** every mutant
//!   fails. A case that passes on its mutant is itself a failing test.
//! - Cases 1-7 are the old crate's defects restated; cases 8-12 (streaming, shutdown, keep-alive, concurrency, HEAD) need a live connection
//!   and are in `live`; cases 13 (path encoding), 14 (limits), 15 (shutdown grace) 16 (edge origin), 17 (TLS termination) and 18 (ALPN and HTTP/2) are HTTPAnt's own rules. [`measure`] turns three of them into the capability matrix (`docs/CAPABILITY-MATRIX.md`).
//!
//! A new adapter is tested by implementing [`Subject`] for it (or wrapping it in [`AdapterSubject`]) and calling [`run_suite`].

mod app;
mod cases;
mod client_suite;
mod h2_case;
mod live;
mod mutants;
pub mod raw;
mod subject;
mod tls_case;

use httpant_core::BoxFuture;

pub use client_suite::{
    CLIENT_CASES, ClientAdapterSubject, ClientCase, ClientMeasured, ClientMutant,
    ClientMutantSubject, ClientSubject, HyperClientSubject, check_client_case, client_adapters,
    client_reference, measure_client, run_client_suite,
};
pub use live::{Measured, measure};
pub use mutants::Mutant;
pub use subject::{AdapterSubject, App, MutantSubject, Setup, Subject, adapters, reference};

/// One conformance case and the broken variants it must be shown able to fail on.
#[derive(Clone, Copy)]
pub struct Case {
    /// Number in the design table (1-based, contiguous).
    pub number: u8,
    /// Stable short id.
    pub id: &'static str,
    /// What a conforming adapter does.
    pub requirement: &'static str,
    /// The deliberately broken variants this case must fail on (each reproduces a defect the survey measured in the old crate).
    pub mutants: &'static [Mutant],
    run: for<'a> fn(&'a dyn Subject) -> BoxFuture<'a, Result<(), String>>,
}

impl Case {
    /// Run this case against `subject`. `Err` carries what went wrong.
    pub async fn run(&self, subject: &dyn Subject) -> Result<(), String> {
        (self.run)(subject).await
    }
}

impl std::fmt::Debug for Case {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Case {{ number: {}, id: {:?} }}", self.number, self.id)
    }
}

/// The sixteen cases. 1-7 are the old `web-server-abstraction` defects restated as tests (survey, "conformance suite's first failing cases").
pub const CASES: [Case; 19] = [
    Case {
        number: 1,
        id: "method-fidelity",
        requirement: "PROPFIND and TRACE reach the handler as sent; an unknown method on a known path gets 405 with Allow, never the GET handler",
        mutants: &[Mutant::FoldMethodToGet],
        run: cases::method_fidelity,
    },
    Case {
        number: 2,
        id: "status-fidelity",
        requirement: "200, 204, 301, 404, 418 and 503 arrive unchanged",
        mutants: &[Mutant::ClampStatus],
        run: cases::status_fidelity,
    },
    Case {
        number: 3,
        id: "middleware-applied",
        requirement: "a layer that adds a header and a layer that short-circuits both take effect on a real request",
        mutants: &[Mutant::NoMiddleware],
        run: cases::middleware_applied,
    },
    Case {
        number: 4,
        id: "headers",
        requirement: "two Set-Cookie headers arrive as two lines; non-UTF8 request header bytes are visible to the handler",
        mutants: &[Mutant::HashMapHeaders],
        run: cases::headers,
    },
    Case {
        number: 5,
        id: "bodies",
        requirement: "a POST body reaches the handler; a request declaring a 10 MB body over a 1 MiB limit is refused with 413; a binary response is byte-identical",
        mutants: &[
            Mutant::EmptyRequestBody,
            Mutant::LossyResponseBody,
            Mutant::TruncateRequestBody,
        ],
        run: cases::bodies,
    },
    Case {
        number: 6,
        id: "request-identity",
        requirement: "the handler sees the real method, path and query",
        mutants: &[Mutant::HardCodeGetRoot],
        run: cases::request_identity,
    },
    Case {
        number: 7,
        id: "path-rule",
        requirement: "`/a` and `/a/` are different paths on every adapter (the one documented rule: no normalisation)",
        mutants: &[Mutant::TrimTrailingSlash],
        run: cases::path_rule,
    },
    Case {
        number: 8,
        id: "streaming",
        requirement: "the first chunk of a response reaches the client while the handler is still producing the rest; the handler sees the first chunk of a request body while the client is still sending",
        mutants: &[Mutant::BufferResponseBody, Mutant::BufferRequestBody],
        run: live::streaming,
    },
    Case {
        number: 9,
        id: "graceful-shutdown",
        requirement: "after shutdown is requested a request in flight completes, an idle keep-alive connection is closed, new connections are refused, and shutdown returns after the request completes",
        mutants: &[Mutant::AbortOnShutdown],
        run: live::graceful_shutdown,
    },
    Case {
        number: 10,
        id: "connection-handling",
        requirement: "two requests share one connection; `Connection: close` is honoured; `Expect: 100-continue` gets `100 Continue` before the body is sent",
        mutants: &[
            Mutant::ForceConnectionClose,
            Mutant::IgnoreConnectionClose,
            Mutant::SkipExpectContinue,
        ],
        run: live::connection_handling,
    },
    Case {
        number: 11,
        id: "concurrency",
        requirement: "1,000 requests, 250 at a time, are each answered with their own response, and fast enough that requests overlap",
        mutants: &[Mutant::SerialiseRequests],
        run: live::concurrency,
    },
    Case {
        number: 12,
        id: "head",
        requirement: "HEAD returns the headers GET returns and no body, and the connection stays usable",
        mutants: &[Mutant::HeadStripsHeaders],
        run: live::head,
    },
    Case {
        number: 13,
        id: "path-encoding",
        requirement: "a percent-encoded path reaches the handler decoded; a bad escape, non-UTF-8, an encoded `/`, backslash, NUL or dot segment is a 400; the encoded spelling of a literal route is a 404",
        mutants: &[Mutant::DecodeBeforeRouting],
        run: cases::path_encoding,
    },
    Case {
        number: 14,
        id: "limits",
        requirement: "the default limits hold: a request target over 8 KiB is 414, more than 64 header fields or over 16 KiB of header bytes is 431, a body over 2 MiB (declared or streamed) is 413; exactly at a limit is served",
        mutants: &[
            Mutant::NoUriLimit,
            Mutant::NoHeaderCountLimit,
            Mutant::NoHeaderBytesLimit,
            Mutant::NoBodyLimit,
        ],
        run: cases::limits,
    },
    Case {
        number: 15,
        id: "shutdown-grace",
        requirement: "a request that never finishes does not hold shutdown for ever: shutdown waits for the grace period, then returns, and the client of the abandoned request gets a 503 or a closed connection",
        mutants: &[Mutant::NoShutdownGrace],
        run: live::shutdown_grace,
    },
    Case {
        number: 16,
        id: "edge-origin",
        requirement: "the client address and scheme a request reports come from the forwarded headers only when the connection is from a configured trusted proxy; from any other peer the headers are ignored",
        mutants: &[Mutant::TrustAnyPeer, Mutant::IgnoreTrustedProxies],
        run: live::edge_origin,
    },
    Case {
        number: 17,
        id: "tls-terminate",
        requirement: "an adapter that declares `tls: Yes` serves HTTPS on TLS 1.3 and 1.2 with the certificate it was given and reports the request as https, and a cleartext client gets no HTTP answer; one that declares `tls: No` refuses to start with a TLS acceptor, so TLS asked for is never served in clear",
        mutants: &[Mutant::ServeTlsInClear],
        run: tls_case::tls_terminate,
    },
    Case {
        number: 18,
        id: "http2-alpn",
        requirement: "a TLS listener offers exactly what the adapter serves: an adapter that declares `http2: Yes` negotiates h2 with a client offering h2 and http/1.1 and answers a request over HTTP/2; one that declares `http2: No` negotiates http/1.1 and answers a request over it, and never offers h2",
        mutants: &[Mutant::OffersH2WithoutServingIt],
        run: h2_case::http2_alpn,
    },
    Case {
        number: 19,
        id: "h2c-switch",
        requirement: "cleartext HTTP/2 (h2c) is off unless asked for (`h2c(true)`), on every adapter whose library can refuse it, and the capabilities say where it cannot: `h2c_refusable: Yes` refuses the HTTP/2 preface by default and `No` accepts it; `h2c: Yes` serves HTTP/2 when asked and `No` fails to start with an error that names h2c; HTTP/1.1 is unaffected in every mode that starts",
        mutants: &[Mutant::H2cOnByDefault, Mutant::IgnoresH2cRequest],
        run: h2_case::h2c_switch,
    },
];

/// Run every case against `subject`: `(case number, result)` for each.
pub async fn run_suite(subject: &dyn Subject) -> Vec<(u8, Result<(), String>)> {
    let mut out = Vec::new();
    for case in &CASES {
        out.push((case.number, case.run(subject).await));
    }
    out
}

/// The both-halves check: `reference` passes the case and **every** listed mutant fails it.
pub async fn check_case(case: &Case, reference: &dyn Subject) -> Result<(), String> {
    case.run(reference).await.map_err(|e| {
        format!(
            "case {} `{}` failed on {}: {e}",
            case.number,
            case.id,
            reference.name()
        )
    })?;
    for &mutant in case.mutants {
        let subject = MutantSubject(mutant);
        if case.run(&subject).await.is_ok() {
            return Err(format!(
                "case {} `{}` PASSED on mutant `{}`: the case cannot fail, so it proves nothing",
                case.number,
                case.id,
                mutant.id()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_are_numbered_from_one_in_order() {
        for (i, c) in CASES.iter().enumerate() {
            assert_eq!(usize::from(c.number), i + 1, "{}", c.id);
        }
    }

    #[test]
    fn every_case_has_an_id_a_requirement_and_a_mutant() {
        let mut ids: Vec<_> = CASES.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CASES.len(), "duplicate case id");
        for c in CASES {
            assert!(
                !c.id.is_empty() && !c.requirement.is_empty() && !c.mutants.is_empty(),
                "case {} is incomplete",
                c.number
            );
        }
    }

    #[test]
    fn every_mutant_is_used_by_exactly_one_case() {
        let all = [
            Mutant::FoldMethodToGet,
            Mutant::H2cOnByDefault,
            Mutant::IgnoresH2cRequest,
            Mutant::ClampStatus,
            Mutant::NoMiddleware,
            Mutant::HashMapHeaders,
            Mutant::EmptyRequestBody,
            Mutant::LossyResponseBody,
            Mutant::TruncateRequestBody,
            Mutant::HardCodeGetRoot,
            Mutant::TrimTrailingSlash,
            Mutant::BufferResponseBody,
            Mutant::BufferRequestBody,
            Mutant::AbortOnShutdown,
            Mutant::ForceConnectionClose,
            Mutant::SerialiseRequests,
            Mutant::HeadStripsHeaders,
        ];
        for m in all {
            let users = CASES.iter().filter(|c| c.mutants.contains(&m)).count();
            assert_eq!(users, 1, "mutant `{}` is used by {users} cases", m.id());
        }
    }

    macro_rules! case_test {
        ($name:ident, $index:expr) => {
            #[tokio::test]
            async fn $name() {
                let case = &CASES[$index];
                if let Err(e) = check_case(case, &reference()).await {
                    panic!("{e}");
                }
            }
        };
    }

    case_test!(case_1_method_fidelity, 0);
    case_test!(case_2_status_fidelity, 1);
    case_test!(case_3_middleware_applied, 2);
    case_test!(case_4_headers, 3);
    case_test!(case_5_bodies, 4);
    case_test!(case_6_request_identity, 5);
    case_test!(case_7_path_rule, 6);
    case_test!(case_8_streaming, 7);
    case_test!(case_9_graceful_shutdown, 8);
    case_test!(case_10_connection_handling, 9);

    /// A mutant that fails case 10 for some other reason (a hang somewhere unrelated) would prove nothing about the part it stands for.
    #[tokio::test]
    async fn the_case_10_mutants_fail_for_their_own_reason() {
        let case = &CASES[9];
        let why = |m| async move { case.run(&MutantSubject(m)).await.unwrap_err() };
        let e = why(Mutant::ForceConnectionClose).await;
        assert!(e.contains("request 2 on one connection"), "{e}");
        let e = why(Mutant::IgnoreConnectionClose).await;
        assert!(
            e.contains("kept the connection open after `Connection: close`"),
            "{e}"
        );
        let e = why(Mutant::SkipExpectContinue).await;
        assert!(e.contains("`100 Continue` before the body was sent"), "{e}");
    }

    case_test!(case_11_concurrency, 10);
    case_test!(case_12_head, 11);
    case_test!(case_13_path_encoding, 12);
    case_test!(case_14_limits, 13);
    case_test!(case_15_shutdown_grace, 14);
    case_test!(case_16_edge_origin, 15);
    case_test!(case_17_tls_terminate, 16);
    case_test!(case_18_http2_alpn, 17);
    case_test!(case_19_h2c_switch, 18);

    #[test]
    fn client_cases_are_numbered_in_order_and_each_mutant_is_used_once() {
        for (i, c) in CLIENT_CASES.iter().enumerate() {
            assert_eq!(usize::from(c.number), i + 1, "{}", c.id);
            assert!(!c.mutants.is_empty(), "{} has no mutant", c.id);
        }
        let all = [
            ClientMutant::FoldMethodToGet,
            ClientMutant::CollapseHeaders,
            ClientMutant::LossyBodies,
            ClientMutant::ClampStatus,
            ClientMutant::BufferResponseBody,
            ClientMutant::BufferRequestBody,
            ClientMutant::NoConnectionReuse,
            ClientMutant::AcceptAnyCertificate,
            ClientMutant::IgnoreHostName,
            ClientMutant::GuardChecksNameOnly,
        ];
        for m in all {
            let users = CLIENT_CASES
                .iter()
                .filter(|c| c.mutants.contains(&m))
                .count();
            assert_eq!(
                users,
                1,
                "client mutant `{}` is used by {users} cases",
                m.id()
            );
        }
    }

    macro_rules! client_case_test {
        ($name:ident, $index:expr) => {
            #[tokio::test]
            async fn $name() {
                let case = &CLIENT_CASES[$index];
                if let Err(e) = check_client_case(case, &client_reference()).await {
                    panic!("{e}");
                }
            }
        };
    }

    client_case_test!(client_case_1_method_fidelity, 0);
    client_case_test!(client_case_2_headers, 1);
    client_case_test!(client_case_3_bodies, 2);
    client_case_test!(client_case_4_statuses, 3);
    client_case_test!(client_case_5_streaming_response, 4);
    client_case_test!(client_case_6_streaming_request, 5);
    client_case_test!(client_case_7_connection_reuse, 6);
    client_case_test!(client_case_8_https, 7);
    client_case_test!(client_case_9_private_guard, 8);

    #[tokio::test]
    async fn the_whole_client_suite_passes_on_every_client_adapter() {
        for subject in &client_adapters() {
            let failed: Vec<_> = run_client_suite(subject.as_ref())
                .await
                .into_iter()
                .filter(|(_, r)| r.is_err())
                .collect();
            assert!(failed.is_empty(), "{}: {failed:?}", subject.name());
        }
    }

    #[tokio::test]
    async fn declared_client_capabilities_match_the_measured_ones() {
        for subject in &client_adapters() {
            let declared = subject.declared().expect("an adapter declares");
            let m = measure_client(subject.as_ref()).await;
            assert_eq!(
                (
                    declared.streaming_request_bodies,
                    declared.streaming_response_bodies,
                    declared.connection_reuse,
                    declared.follows_redirects,
                    declared.tls
                ),
                (
                    m.streaming_request_bodies,
                    m.streaming_response_bodies,
                    m.connection_reuse,
                    m.follows_redirects,
                    m.tls
                ),
                "{}: declared client capabilities differ from what the suite measured",
                subject.name()
            );
        }
    }

    #[tokio::test]
    async fn declared_capabilities_match_the_measured_ones() {
        for subject in &adapters() {
            let declared = subject.declared().expect("an adapter declares");
            let measured = measure(subject.as_ref()).await;
            assert_eq!(
                (
                    declared.streaming_request_bodies,
                    declared.streaming_response_bodies,
                    declared.graceful_shutdown,
                    declared.h2c,
                    declared.h2c_refusable
                ),
                (
                    measured.streaming_request_bodies,
                    measured.streaming_response_bodies,
                    measured.graceful_shutdown,
                    measured.h2c_when_asked,
                    // Refusable is the inverse of "accepted the preface although nothing asked for it".
                    if measured.h2c_by_default == httpant_core::Support::Yes {
                        httpant_core::Support::No
                    } else {
                        httpant_core::Support::Yes
                    }
                ),
                "{}: declared capabilities differ from what the suite measured",
                subject.name()
            );
        }
    }

    /// `docs/CAPABILITY-MATRIX.md` holds the table this renders. `HTTPANT_BLESS=1 cargo test` rewrites it.
    #[tokio::test]
    async fn the_capability_matrix_document_is_current() {
        let mut rows = String::new();
        for subject in &adapters() {
            let m = measure(subject.as_ref()).await;
            let declared = subject.declared().expect("an adapter declares");
            let cell = |s: httpant_core::Support| match s {
                httpant_core::Support::Yes => "yes",
                httpant_core::Support::No => "no",
                httpant_core::Support::Unknown => "unknown",
            };
            rows.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                subject
                    .name()
                    .trim_matches('`')
                    .replace("adapter `", "")
                    .replace('`', ""),
                cell(m.streaming_request_bodies),
                cell(m.streaming_response_bodies),
                cell(m.graceful_shutdown),
                cell(declared.local_handlers),
                cell(declared.tls),
                cell(declared.http2),
                cell(m.h2c_by_default),
                cell(m.h2c_when_asked),
                declared.default_body_limit,
            ));
        }
        let table = format!(
            "| adapter | streaming request bodies | streaming response bodies | graceful shutdown | non-`Send` handlers | terminates TLS | HTTP/2 (over TLS) | cleartext h2c by default (measured) | cleartext h2c when asked (measured) | default body limit (bytes) |\n|---|---|---|---|---|---|---|---|---|---|\n{rows}"
        );
        let mut client_rows = String::new();
        for subject in &client_adapters() {
            let m = measure_client(subject.as_ref()).await;
            let declared = subject.declared().expect("an adapter declares");
            let cell = |s: httpant_core::Support| match s {
                httpant_core::Support::Yes => "yes",
                httpant_core::Support::No => "no",
                httpant_core::Support::Unknown => "unknown",
            };
            client_rows.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                subject.name(),
                cell(m.streaming_request_bodies),
                cell(m.streaming_response_bodies),
                cell(m.connection_reuse),
                cell(m.follows_redirects),
                cell(m.tls),
                declared.default_body_limit,
            ));
        }
        let client_table = format!(
            "| client adapter | streaming request bodies | streaming response bodies | connection reuse | follows redirects | TLS | default body limit (bytes) |\n|---|---|---|---|---|---|---|\n{client_rows}"
        );
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/CAPABILITY-MATRIX.md"
        );
        let current = std::fs::read_to_string(path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        let mut updated = current.clone();
        for (name, table) in [("matrix", &table), ("client-matrix", &client_table)] {
            let start = format!("<!-- {name}:start -->\n");
            let end = format!("<!-- {name}:end -->");
            let (Some(a), Some(b)) = (updated.find(&start), updated.find(&end)) else {
                panic!("docs/CAPABILITY-MATRIX.md has no `{name}` markers");
            };
            updated = format!("{}{start}{table}{}", &updated[..a], &updated[b..]);
        }
        if std::env::var_os("HTTPANT_BLESS").is_some() {
            std::fs::write(path, &updated).unwrap();
            return;
        }
        assert_eq!(
            current, updated,
            "docs/CAPABILITY-MATRIX.md is out of date: run `HTTPANT_BLESS=1 cargo test -p httpant-conformance`"
        );
    }

    #[tokio::test]
    async fn the_whole_suite_passes_on_every_adapter() {
        let all = adapters();
        assert!(all.len() >= 2, "expected hyper and axum, got {}", all.len());
        for subject in &all {
            let results = run_suite(subject.as_ref()).await;
            assert_eq!(results.len(), CASES.len());
            let failed: Vec<_> = results.iter().filter(|(_, r)| r.is_err()).collect();
            assert!(failed.is_empty(), "{}: {failed:?}", subject.name());
        }
    }
}
