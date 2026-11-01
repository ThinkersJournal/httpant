# Cleartext HTTP/2 (h2c) policy (ROADMAP row 13)

**Decided 2026-11-01 (PM) and built in 0.5.35: option C.** `Server::h2c(true)` asks for it; off by default; warp cannot refuse it and is declared
so (`h2c_refusable: No`). What follows is the analysis that led to it; the measured result is `docs/CAPABILITY-MATRIX.md`.
One correction to the text below: the guard does not read ahead or hold bytes back (it passes every byte through and ends the connection when the
first 14 bytes spell `PRI * HTTP/2.0`), so it adds no delay for a slow client.

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

The PM ruled on 2026-11-01: adopt C (opt-in, off by default); finish step 1 first, then build the default-off switch where a library can refuse and
say plainly in the matrix where it cannot.

## Step 1: can each library refuse the preface? (read 2026-11-01, not run)

Read from the sources in `~/.cargo/registry` at the versions in `Cargo.lock` (axum 0.8.9, poem 3.1.12, salvo_core 1.0.1, warp 0.4.3, hyper-util
0.1.21). **Nothing was compiled or run**; each "no setting" is a grep that found none (control: the same greps find `http1_only` in hyper-util's
`auto::Builder` and `http2_mut` in salvo, which are there).

| library | how it picks the protocol | a setting to refuse h2c? | an HTTPAnt-side way |
|---|---|---|---|
| axum 0.8.9 | `axum::serve` builds `hyper_util::server::conn::auto::Builder::new(..)` inside the task it spawns per connection (`serve/mod.rs`), enabling the h2 CONNECT protocol with the `http2` feature | **none found**: the builder is not reachable | wrap the listener (axum's `Listener` trait is public and HTTPAnt already wraps it for TLS, `tls_listener.rs`): the wrapper's I/O type would look at the first 24 bytes and refuse a connection that starts with the preface. Idea, unbuilt |
| poem 3.1.12 | `server.rs` builds `auto::Builder`, then configures `.http2()` limits only | **none found** | wrap poem's `Acceptor` (public; HTTPAnt implements it for TLS) the same way |
| salvo_core 1.0.1 | its own `conn/proto.rs` compares the first bytes with `H2_PREFACE` and serves HTTP/2 or HTTP/1; `HttpBuilder` exposes `http1_mut`/`http2_mut` | **none at run time**: `http2_mut` tunes HTTP/2, it does not switch it off; the `http2` cargo feature is all-or-nothing (also removes `h2` over TLS) | wrap salvo's `Acceptor` (public; HTTPAnt wraps it for TLS) |
| warp 0.4.3 | `server.rs` builds `auto::Builder` and calls `.http1()` (tuning the HTTP/1 side), which does not remove h2 detection | **none found** | **none**: `Accept` is in a private module (the same `E0603` as for TLS, `docs/TLS-ADAPTER-PROBES.md`) so HTTPAnt cannot wrap warp's accept loop |

What this means for the build step: a **default-off switch is plausible on axum, poem and salvo** by wrapping their accept path so a connection whose
first bytes are the preface is refused before the library sees them (one shared helper, not three), **not possible on warp** with warp 0.4.3, and
already so on hyper (`http1_only`) and actix-web (no HTTP/2). Warp would then be declared `h2c: yes` (it cannot be turned off) and the matrix
and `README` must say so plainly. The wrapper has a cost to measure when built: every cleartext connection pays a read of up to 24 bytes before the
library sees it, and a slow client that sends fewer than 24 bytes must not be held longer than the library would have held it (the library's own
header timeout, if any, starts later). That is the case the build must add a mutant for.

The pieces **not** checked: that the wrapper really stops each library from answering h2c (it must be probed, and the existing probe
`h2_case::h2c_support` is the check); that wrapping does not disturb graceful shutdown or the trusted-proxy `Origin` path.
