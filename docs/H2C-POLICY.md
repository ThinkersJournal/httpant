# Cleartext HTTP/2 (h2c) policy (ROADMAP row 13)

Options first, no code. Written 2026-10-31 at `origin/main` `ea3483b`. **Measured** below means a row of `docs/CAPABILITY-MATRIX.md` (the probe
`h2_case::h2c_support`, run with the `http2` feature on); **read** means I read the source and did not run it; **unchecked** is said where it applies.

## What h2c is here

A client that already knows the server speaks HTTP/2 sends the HTTP/2 connection preface (`PRI * HTTP/2.0 ...`) on a plain TCP connection, with
no TLS and no `Upgrade`. It matters to HTTPAnt in one place: **a program behind an edge that terminates TLS**. The edge can then talk HTTP/2 to the
origin only if the origin accepts h2c. Over TLS, HTTP/2 is chosen by ALPN and is uniform (case 18).

## Measured today

| adapter | accepts the h2c preface on a cleartext listener | measured/read |
|---|---|---|
| hyper | **no** (`http1_only()` is set on a cleartext connection, `hyper_adapter.rs`, a deliberate choice) | measured, and read |
| axum | yes | measured |
| actix-web | no (no HTTP/2 at all, `http2: no`) | measured |
| poem | yes | measured |
| salvo | yes (its connection builder detects the preface on any stream once its own `http2` feature is on; it moved from `no` to `yes` when 0.5.14 turned that on) | measured |
| warp | yes | measured |

Not measured and therefore not claimed: whether the body limits, header limits, the HTTPS redirect, the ACME prefix and the trusted-proxy
`Origin` behave the same over h2c as over HTTP/1.1 on the four adapters that accept it. They are wrappers around the `Send` service
(`ServeOptions::wrap`), which suggests they do, but no case exercises h2c, so that is a reading and not a result. Without the `http2` feature
of `httpant-server`, no adapter was probed; the difference exists only with it on.

The need list (WebHostAnt) does not ask for h2c, so conformance does not constrain it; the PM ruled on 2026-10-31 that the difference is
recorded, not hidden. This document is the decision that ruling left open.

## Options

### A. Refuse h2c on every adapter (default off, no way to turn it on)

hyper is already so. axum, poem, salvo and warp would need their cleartext listener to stop accepting the preface.

- **Cost / unchecked:** whether each of the four libraries' builders can be told to refuse (axum, poem and warp through their hyper-util
  builder, salvo through its own) was **not read**. If one cannot, the fallback is HTTPAnt peeking the first bytes itself, which only hyper
  (HTTPAnt's own accept loop) could do; the other adapters' libraries own theirs. That is a real risk to this option.
- **Compatibility:** an edge configured for h2c to an axum, poem, salvo or warp origin stops working (connection errors, not a downgrade).
  This is a behaviour change on four adapters, so a breaking release (second number, per the portfolio rule).
- **Gain:** one answer, one case, no per-adapter footnote; no edge can reach an HTTP/2 stack that HTTPAnt's own limits have not been measured on.
- **Loss:** a user behind an edge cannot use HTTP/2 to the origin at all.

### B. Leave it per adapter (status quo, document only)

- **Compatibility:** nothing changes, no release is breaking.
- **Cost:** swapping the adapter changes the wire behaviour of a program that did nothing different (hyper refuses what axum accepts), the
  exact thing a library of interchangeable adapters exists to avoid. Anyone who needs h2c must pick among four adapters by reading a matrix
  column, and the matrix says only that it differs.
- **Risk, not tested here:** a front proxy that inspects only HTTP/1.1 traffic can be bypassed by a client that reaches an origin directly
  and speaks h2c (a known class of "h2c smuggling"). Whether any of our deployments has such a proxy is outside this repository.

### C. Opt-in, off by default, refused by name where unsupported (recommended)

A `ServeOptions` / `Server` setting (name open, for example `.h2c(true)`) that is **off by default on every adapter** and, when on, makes a
cleartext listener accept the prior-knowledge preface. An adapter that cannot do it returns a named startup error, the same rule as
`LocalHandler` and TLS (never a silent downgrade). A capability `h2c: Support` declared per adapter and held to its declaration by a new case
(as case 18 does for ALPN), with the existing probe as the check of "on" and of "off".

- **Compatibility:** uniform default, so two adapters behave alike. **Breaking only for axum, poem, salvo and warp users who rely on h2c
  today** (the default flips for them); they add one call. hyper and actix-web users see no change unless they turn it on.
- **Cost:** a code change per adapter and the same unread question as option A for the four libraries whose builders must be told to
  refuse by default (so "off" is as hard as in A); hyper's side is small (`http1_only()` only when the flag is off). actix-web answers by
  name (it has no HTTP/2). Cases needed: default-off refuses on all, on accepts where declared, and the limits/redirect/ACME/Origin wrappers still apply over h2c
  (the unmeasured row above becomes measured).
- **Gain:** edge users who want h2c to the origin can have it, deliberately, with the protections held by a case.

## Recommendation

**C**, built in two steps so the unknown is settled first: (1) read whether axum, poem, salvo and warp can refuse the preface on a cleartext
listener (a read-and-probe PR, no behaviour change; if one cannot, say so and keep that adapter's `h2c` refused by name), then (2) the
setting, the capability and the case. If step 1 shows that "off" is impossible on more than one library, fall back to **B** with the
column kept, because A would then be unbuildable.

Nothing here is built and nothing was run for this document; the table is the matrix's measurement plus the `hyper_adapter.rs` source.
