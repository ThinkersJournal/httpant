# Patent sweep: HTTPAnt (httpant, httpant-core, httpant-server, httpant-client; line 0.5.x) @ 2d8a670 (0.5.1)

Date: 2026-10-30; Run by: HTTPAnt lane, Claude Sonnet 5.5; Method: AI-assisted search and read (web search over Google Patents and Justia, then claims read with `tools/patent-claims.py`); **NOT legal advice; NOT a freedom-to-operate opinion.**

Covers the `0.5.x` line at commit `2d8a670` (main after PR #21). It also covers the code of the old `web-server-abstraction` 1.0.x only insofar as it is the same idea (idea 1). A new family of capability (HTTP/2, the ACME challenge handler, a client TLS stack) needs a re-sweep (PATENT-SWEEP-PROCESS.md).

## Ideas swept

1. One set of routes and Tower-style layers run unchanged on interchangeable web libraries chosen by an identifier, with the library behind a thin adapter that only converts requests and responses.
2. Verifying each adapter with a conformance suite over real sockets, and checking that the suite can fail by running it against deliberately broken adapters (mutants).
3. Limits enforced by the layer above the adapter on the request each adapter hands over (target length, header count and bytes, body size), so they mean the same on every library; refusal on the head from `Content-Length`, and a streaming read that fails once it passes the limit.
4. Percent-decoding of path captures only, with literals matched raw, and refusal of unsafe encodings (encoded separator, dot segments, NUL, invalid UTF-8) before any handler runs.
5. Bounded graceful shutdown: stop accepting, let requests in flight finish, and after a grace period end the ones still running (drop the handler, answer 503 with `Connection: close`) as a Tower wrapper, the same on every library.
6. Client address and scheme taken from forwarded headers only when the peer is in a configured trusted-proxy set; the client is the rightmost `X-Forwarded-For` hop that is not itself trusted; nobody trusted by default.
7. A certificate source called once per TLS handshake with the SNI name (or none), so a replaced certificate serves the next connection; an unknown name or a failing source refuses the handshake and never falls back to another name's certificate.
8. Private key held in a type whose `Debug` and errors never show it.
9. A capability table per adapter (`Yes`/`No`/`Unknown`) filled by the conformance run, with TLS requested on an adapter that cannot do it refused at start-up rather than served in clear.
10. Handshakes run outside the accept path (own task per handshake, bounded by a timeout), so a silent client holds only its own slot.

## Queries (source, query, date)

All 2026-10-30, web search (US), standard mode. Search snippets are not claims; only the claims read below were relied on.

| # | idea | source | query |
|---|---|---|---|
| Q1 | 1 | web | patent web application abstraction layer interchangeable web server frameworks adapter single set of routes middleware |
| Q2 | 1 | patents.google.com, patents.justia.com | web application framework independent adapter plurality of web server frameworks common request response interface route middleware |
| Q3 | 2 | web | patent conformance test suite deliberately faulty implementations mutants verify adapters software interface |
| Q4 | 7 | web | patent TLS certificate callback per handshake server name indication certificate source pluggable renewal without restart |
| Q5 | 7, 10 | patents.google.com, patents.justia.com | server name indication select certificate from certificate provider during handshake asynchronous callback hello |
| Q6 | 6 | web | patent trusted proxy X-Forwarded-For rightmost untrusted hop client address determination |
| Q7 | 6 | patents.google.com, patents.justia.com | forwarded header client address trusted proxy list reverse proxy request originating address determining |
| Q8 | 5 | web | patent graceful shutdown grace period forced termination in-flight requests server HTTP |
| Q9 | 5 | patents.google.com, patents.justia.com | server graceful shutdown drain connections timeout terminate remaining requests |
| Q10 | 3 | patents.google.com, patents.justia.com | HTTP request limits request line header count body size reject 413 431 before handler |

Ideas 4, 8 and 9 were not searched on their own: 4 is covered by the search for 3 (limits and path handling are one family, nothing relevant surfaced); 8 and 9 are ordinary engineering with no distinguishing mechanism worth a query. That is a gap, not a clearance.

## Hits

Claims read in full with `tools/patent-claims.py`. Status is Google Patents', not the official record.

| # | patent | title | assignee | status | claim(s) read | overlap | our difference | verdict |
|---|---|---|---|---|---|---|---|---|
| H1 | US6233622B1 | Adapter and handler framework for web server extensions | IBM | Expired (priority 1996-11-12) | 1, 9, 17, 25, 33, 39 | An adapter between a web server and extensions | Claims create request/reply/handler objects from subclasses chosen by the identified interface type; HTTPAnt has one service and no class-per-interface hierarchy | clear (expired, and no overlap on any independent claim) |
| H2 | US20050149941A1 | Framework for adapter used in enterprise application integration | Information Builders | Expired (priority 2004-01-05) | 1 | Adapter mediating between two applications | EAI adapter-production framework with configured interfaces both ways; not a web request service | clear (expired, no overlap) |
| H3 | US10705949B2 | Evaluation of library test suites using mutation testing | Oracle | Active (est. expiry 2038) | 1, 8, 12 | Mutants used to judge a test suite | Every independent claim requires a security call in a library class modified to check a second permission; we mutate adapters (e.g. skip a limit) to prove a conformance case can fail, with no security-call change | clear |
| H4 | US20210111907A1 | Automated replacement of renewable server certificates | Red Hat | Application, active (est. 2040) | 1, 8, 15 | Certificate replacement at session start | Claims detect expiry, generate a CSR and fetch from a CA; HTTPAnt never does ACME, CSR, storage or renewal (WebHostAnt's side) and only asks a source for a certificate | clear (by design: this is the line HTTPAnt must not cross) |
| H5 | US8738902B2 | Implicit SSL certificate management without SNI | Microsoft | Expired (fee, est. 2032) | 1 | Handling a hello with no hostname | Claim maps IP:port to hostnames and builds a search name from one; HTTPAnt passes `None` to the source and does no IP:port mapping | clear; **re-check if a no-SNI IP:port mapping is ever added** |
| H6 | US20140258465A1 | Identification of originating IP address and client port connection to a web server via a proxy | Cisco | Abandoned | 1 | Conveying client address through a proxy | Claim: a proxy embeds an "XFS" value (TCP header); HTTPAnt is the receiver and reads the standard `X-Forwarded-For` | clear |
| H7 | US20050021771A1 | System enabling server progressive workload reduction to support server maintenance | Individual / Cerner | Expired | 1 | Draining a server | Claim 1 is a URL-parsing switch directing new sessions to a probed server; no in-process drain/force | clear |

## Not read

Q3 (conformance suites of faulty implementations), Q6 and Q8 (trusted-proxy walking, bounded shutdown) and Q10 (request limits) surfaced no patent addressing the idea; the results were documentation and articles (the trusted-proxy walk and the grace-period-then-force pattern are in MDN, gRPC, Kestrel and Armeria documentation, which is prior art for ideas 5 and 6, not a patent). Several Q5 and Q9 results (routing-protocol graceful shutdown; TLS-proxy patents US8327128B1 and US20170302700A1) were judged out of scope from their titles and abstracts and were **not** read: they are `unread`, not `clear`.

## Needs review (to the PM)

No hit has an unresolved overlap. The PM should decide whether the limits below are enough for a first release of the `0.x` line: the search was one pass over a general web index, not a boolean search of USPTO, EPO or WIPO with classification codes, and ideas 4, 8 and 9 got no query.

## Limitations

AI-assisted due-diligence read; not legal advice; not a freedom-to-operate opinion. Ten queries through a general web search that returns a handful of Google Patents and Justia pages each, so coverage of the patent literature is thin and an absence of hits is weak evidence. Seven patents had their claims read; the machine-converted claim text should be checked against the official record before any quote is relied on. Status and expiry are Google Patents' estimates. The sweep describes the code at `2d8a670`; adapters for libraries other than the six in tree, HTTP/2, and an ACME or client TLS stack are outside it.
