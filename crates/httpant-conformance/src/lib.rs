// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTPAnt adapter conformance suite (design `docs/HTTPANT-DESIGN.md`, section 6).
//!
//! - [`CASES`]: the first seven cases, each run over a **real socket** against a [`Subject`] (an adapter), speaking through [`raw`], the suite's own
//!   minimal HTTP/1.1 client. The client is deliberately not HTTPAnt, hyper or any adapter, so a defect cannot hide on both ends of a test.
//! - Every case lists the known-bad [`Mutant`]s it must fail on. [`check_case`] demands both halves: the reference passes **and** every mutant
//!   fails. A case that passes on its mutant is itself a failing test.
//! - Cases 8-12 of the design (streaming, shutdown, keep-alive, concurrency, HEAD) are not written yet.
//!
//! A new adapter is tested by implementing [`Subject`] for it (or wrapping it in [`AdapterSubject`]) and calling [`run_suite`].

mod app;
mod cases;
mod mutants;
pub mod raw;
mod subject;

use httpant_core::BoxFuture;

pub use mutants::Mutant;
pub use subject::{AdapterSubject, App, MutantSubject, Subject, reference};

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

/// The first seven cases: the old `web-server-abstraction` defects, restated as tests (survey, "conformance suite's first failing cases").
pub const CASES: [Case; 7] = [
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
    fn cases_are_numbered_one_to_seven_in_order() {
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
            Mutant::ClampStatus,
            Mutant::NoMiddleware,
            Mutant::HashMapHeaders,
            Mutant::EmptyRequestBody,
            Mutant::LossyResponseBody,
            Mutant::TruncateRequestBody,
            Mutant::HardCodeGetRoot,
            Mutant::TrimTrailingSlash,
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

    #[tokio::test]
    async fn the_whole_suite_passes_on_the_hyper_adapter() {
        let results = run_suite(&reference()).await;
        assert_eq!(results.len(), CASES.len());
        let failed: Vec<_> = results.iter().filter(|(_, r)| r.is_err()).collect();
        assert!(failed.is_empty(), "{failed:?}");
    }
}
