//! HTTPAnt adapter conformance suite: SKELETON (design `docs/HTTPANT-DESIGN.md`, section 6).
//!
//! What exists in this slice:
//! - [`CASES`]: the registry of the first seven cases, each with the known-bad *mutant* it must fail on.
//! - [`raw`]: the suite's own minimal HTTP/1.1 client over a bare TCP socket. It is deliberately **not** HTTPAnt, hyper or any adapter, so a
//!   defect cannot hide on both ends of a test.
//!
//! What does not exist yet: running a case. A case needs a server to run against, which arrives with the first adapter; until then the
//! registry says so by [`Status::Pending`] rather than by an ignored test. Nothing here claims any adapter passes anything.

pub mod raw;

/// Whether a case can be run yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Written down, not runnable until an adapter exists to run it against.
    Pending,
}

/// One conformance case and the broken variant it must be shown able to fail on.
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// Number in the design table (1-based, contiguous).
    pub number: u8,
    /// Stable short id.
    pub id: &'static str,
    /// What a conforming adapter does.
    pub requirement: &'static str,
    /// The deliberately broken adapter behaviour this case must fail on (the old crate's measured defect).
    pub mutant: &'static str,
    /// Whether the case can run yet.
    pub status: Status,
}

/// The first seven cases: the old `web-server-abstraction` defects, restated as tests (survey, "conformance suite's first failing cases").
pub const CASES: [Case; 7] = [
    Case {
        number: 1,
        id: "method-fidelity",
        requirement: "PROPFIND and TRACE reach the handler as sent; an unknown method on a known path gets 405, never the GET handler",
        mutant: "folds unknown methods to GET",
        status: Status::Pending,
    },
    Case {
        number: 2,
        id: "status-fidelity",
        requirement: "200, 204, 301, 404, 418 and 503 arrive unchanged",
        mutant: "maps statuses outside a short list to 200",
        status: Status::Pending,
    },
    Case {
        number: 3,
        id: "middleware-applied",
        requirement: "a layer that adds a header and a layer that short-circuits both take effect on a real request",
        mutant: "stores middleware and never runs it",
        status: Status::Pending,
    },
    Case {
        number: 4,
        id: "headers",
        requirement: "two Set-Cookie headers arrive as two lines; non-UTF8 request header bytes are visible to the handler",
        mutant: "headers held in a HashMap<String,String>",
        status: Status::Pending,
    },
    Case {
        number: 5,
        id: "bodies",
        requirement: "a POST body reaches the handler; a 10 MB body is refused at the limit with 413; a binary response is byte-identical",
        mutant: "empty request body; lossy UTF-8 response; unbounded read",
        status: Status::Pending,
    },
    Case {
        number: 6,
        id: "request-identity",
        requirement: "the handler sees the real method, path and query",
        mutant: "hard-codes GET and `/`; drops the query",
        status: Status::Pending,
    },
    Case {
        number: 7,
        id: "path-rule",
        requirement: "`/a` and `/a/` are different paths on every adapter (the one documented rule: no normalisation)",
        mutant: "trims the trailing slash",
        status: Status::Pending,
    },
];

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
                !c.id.is_empty() && !c.requirement.is_empty() && !c.mutant.is_empty(),
                "case {} is incomplete",
                c.number
            );
        }
    }
}
