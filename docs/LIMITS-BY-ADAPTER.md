# Connection and read limits, by adapter

HTTPAnt's reason to exist is one safe-by-default surface over several web libraries. This is what each adapter does about the three limits
that bound a slow or greedy client, which of them HTTPAnt **enforces** (the same on every adapter that says so) and which it **inherits** from
the library underneath, and what the library does by default. ROADMAP row 17, httpant#73.

| limit | HTTPAnt's setting (default) | hyper | axum | actix-web | poem | salvo | warp |
|---|---|---|---|---|---|---|---|
| request body read timeout (idle time with no data while the body is waited for) | `Limits::body_read_timeout` (60 s, `Duration::ZERO` = off) | enforced | enforced | enforced | enforced | enforced | enforced |
| open connections | `Limits::max_connections` (10,000, `0` = off) | enforced | enforced | enforced (per worker, rounded up to an even total) | enforced | enforced | **inherited: none** (warp's accept loop is private; no cap seen at 3000 connections) |
| request head (header) read timeout, and idle keep-alive | not yet a HTTPAnt limit (row 17, next slice) | inherited: **none** | inherited: **none** | inherited: 5 s (`client_request_timeout`) | inherited: **none** | inherited: 30 s | inherited: **none** |

"Enforced" means HTTPAnt's own code on the one service (the body timeout) or on the accept loop it controls (connections), held by a
conformance case on every adapter: case 21 (body read timeout) and case 22 (connection limit; an adapter that declares
`connection_limit: No` is held to the opposite, so the declaration cannot go stale).

## How the defaults were measured

Windows 11, loopback, 2026-11-12, HTTPAnt at the head of the row 17 branch with no HTTPAnt limit set on the header phase, a plain
`GET /` server per adapter, a window of 40 s, all six at once:

- *stalled head*: connect, send half a request head, then say nothing: hyper, axum, poem and warp were still open at 40 s; salvo closed the
  connection at 30.0 s; actix-web at 5.0 s.
- *idle keep-alive*: one request answered, the connection then left open: the same split (still open at 40 s; salvo 30.0 s; actix-web 5.0 s).
- *many connections*: 3000 idle connections held, then one more client: served by all six (no cap in HTTPAnt or in the libraries at that
  count; axum failed one of the 3000 connects, probably the accept queue, not a limit).

A measurement at a point in time on one machine: a library release can change its default, which is why the table says "inherited".

## What the limits do and do not bound

- **Body read timeout** is an idle time, not a total: an upload that keeps moving is never cut. It bounds how long a client can hold a handler by
  stalling mid-body; a client that sends one byte every 59 s is not caught by an idle time (the drain's own `drain_timeout` bounds the
  refusal case).
- **Connection limit** pauses accepting at the limit: clients wait in the operating system's queue (its backlog) until a connection closes,
  and nothing is refused. Idle keep-alive connections hold slots, so until the header and idle timeouts exist a client that opens the limit's
  worth of connections and does nothing blocks the rest on every adapter that has no timeout of its own (see the last row).
- **Open files (Unix):** each connection is a file descriptor. If the soft limit is lower than `max_connections` plus a reserve of 256,
  HTTPAnt raises the soft limit to what it needs, never past the hard limit; if the hard limit cannot hold it, `bind` fails at start with
  `Error::ConnectionLimitTooHigh` and the numbers, and does not wait to fail under load. Windows has no per-process limit to check.
- **TLS listeners** take the slot before the handshake and keep it for the connection, handshake included (hyper, axum, poem, salvo, actix-web);
  the cleartext case 22 is the test that runs, the TLS paths are not separately tested for the limit.
