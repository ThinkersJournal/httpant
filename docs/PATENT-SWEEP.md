# Patent sweep: HTTPAnt (httpant, httpant-core, httpant-server, httpant-client; line 0.5.x) @ 2d8a670 (0.5.1); addendum @ 340e667 (0.5.2); re-sweep @ 6d564a0 (0.5.22)

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

## Not read (first pass; the two TLS-proxy patents were read in the addendum)

Q3 (conformance suites of faulty implementations), Q6 and Q8 (trusted-proxy walking, bounded shutdown) and Q10 (request limits) surfaced no patent addressing the idea; the results were documentation and articles (the trusted-proxy walk and the grace-period-then-force pattern are in MDN, gRPC, Kestrel and Armeria documentation, which is prior art for ideas 5 and 6, not a patent). Several Q5 and Q9 results (routing-protocol graceful shutdown; TLS-proxy patents US8327128B1 and US20170302700A1) were judged out of scope from their titles and abstracts and were **not** read: they are `unread`, not `clear`.

## Needs review (to the PM)

No hit has an unresolved overlap. The PM should decide whether the limits below are enough for a first release of the `0.x` line: the search was one pass over a general web index, not a boolean search of USPTO, EPO or WIPO with classification codes, and ideas 4, 8 and 9 got no query.

## Addendum 2026-10-30 (second pass, at 340e667 = 0.5.2; code unchanged since 2d8a670, docs only)

Done on the PM's ruling that ten general-web queries do not meet PATENT-SWEEP-PROCESS.md step 2. **What this pass did and did not do, exactly:**

- **Google Patents.** The search tool this lane has is a general web search that can be limited to a domain; it is not Google Patents' own query interface (no boolean, no classification codes, no date or assignee filters), but with `allowed_domains: patents.google.com` it returns Google Patents pages. Queries A1-A5 below.
- **Espacenet, WIPO PatentScope, USPTO.** The same tool limited to `worldwide.espacenet.com`, `patentscope.wipo.int` and `ppubs.uspto.gov`, queries B1-B8. It returned PatentScope record pages, USPTO full-text PDF titles and, from Espacenet, one unrelated record. **Espacenet's own search was not usable from this lane**, so no Espacenet search was performed in the sense of its classification search; the PatentScope/USPTO results were mostly irrelevant (cryptography, wireless, media proxies). Treat B1-B8 as having found nothing, with low confidence that anything could have been found.
- Claims read in this pass with `tools/patent-claims.py`: US8327128B1, US20170302700A1 (the two that were unread), US7496962B2 and US12645555B1 (closest new hits). `US11397789`, `US11636170` (URL normalisation) and `US8683357` were looked at by title/abstract/claim 1 only (US8683357 claim 1 read; the two URL-normalisation applications could not be fetched, kind code unknown: **unread**).

| # | idea | source | query |
|---|---|---|---|
| A1 | 4 | patents.google.com | URL path percent-decoding normalization reject encoded slash dot segments before routing request |
| A2 | 8 | patents.google.com | private cryptographic key redacted from logs debug output error messages secret value type masking |
| A3 | 9 | patents.google.com | server capability declaration per adapter determined by test run refuse start when feature unsupported rather than insecure fallback |
| A4 | 2 | patents.google.com | conformance test suite interchangeable implementations broken implementations must fail test validity |
| A5 | 6 | (done in the first pass as Q7; trusted-proxy, shutdown and limits as Q7, Q9, Q10) | |
| B1 | 4 | espacenet, patentscope, ppubs | URL path percent-decoding normalization reject encoded slash dot segments routing |
| B2 | 8 | same three | secret private key value redacted in debug output logging error messages |
| B3 | 9 | same three | capability declaration adapter refuse to start when requested feature unsupported instead of fallback without encryption |
| B4 | 2 | same three | conformance tests deliberately broken implementations verify test suite detects faults interchangeable adapters |
| B5 | 6 | same three | X-Forwarded-For header trusted proxy addresses client IP determination web server |
| B6 | 5 | same three | graceful shutdown web server in-flight requests grace period then terminate remaining |
| B7 | 3 | same three | HTTP server maximum request header size count body size limit reject request too large before application (no results) |
| B8 | 1 | same three | web server framework portable across multiple server implementations adapters common API middleware routes |
| B9 | 7 | same three | TLS handshake certificate provider called per connection with server name replace certificate new connections without restart (one irrelevant result) |

All queries run 2026-10-30. Ideas 1 to 9 now each have at least one Google Patents query and at least one query on the other three sources.

Hits read in this pass:

| # | patent | title | assignee | status | claim(s) read | overlap | our difference | verdict |
|---|---|---|---|---|---|---|---|---|
| H8 | US8327128B1 | Supporting secure sessions in a cloud-based proxy service | Cloudflare | Active (est. 2032) | 1, 7 | Terminating TLS with a certificate | A proxy that terminates the client's session with one certificate bound to several domains, decrypts, and opens a second secure session to an origin and re-encrypts; HTTPAnt terminates and serves the request itself, with no origin re-encryption and one certificate per name | clear |
| H9 | US20170302700A1 | Providing forward secrecy in a terminating TLS connection proxy | Akamai | Application, active | 1 | TLS termination with a private key | Claim requires the key to be held remote from the server machine and the master secret recomputed from an ephemeral value; HTTPAnt holds the key in the same process and uses rustls' standard handshake | clear |
| H10 | US7496962B2 | Intrusion detection strategies for hypertext transport protocol | Sourcefire/Cisco | Expired (fee; est. 2026) | 1 | Decoding obfuscated URIs | An intrusion-detection inspection engine with policy selection per web server; HTTPAnt is not an inspection engine and refuses unsafe encodings in a router | clear |
| H11 | US12645555B1 | Deterministic conformance suites, validator implementation certification, replay-verifiable conformance receipts | Stone & Ink Archive | Active (priority 2026-01-09) | 1 | A conformance suite run against an implementation | Claim 1 binds a versioned profile to a policy pack identifier and digest and verifies replay-verifiable receipts and certificate registries; the HTTPAnt suite has none of these | clear |
| H12 | US8683357B2 | Providing real time web application framework socket | Red Hat | Active (est. 2032) | 1 | A web application framework | Browser-side socket for widgets; unrelated mechanism | clear |

Nothing in the second pass changes any verdict; no hit needs review. **Still not done:** a classification-code search (for example CPC H04L 67, H04L 9/32, G06F 11/36) on USPTO, EPO or WIPO; any pass by a person with the patent offices' own search interfaces; reading of the two URL-normalisation applications. The PM can accept the file as an honest record of a thin AI-assisted search, or ask for a person with those tools.

## Re-sweep 2026-10-31 (at 6d564a0 = 0.5.22): HTTP/2, the ACME handlers, client https, the private-address guard

**How it came about, in the order it happened.** The first sweep's own scope line (above) said that HTTP/2, the ACME challenge handler and a client TLS stack need a re-sweep. That re-sweep was not done before releases: **0.5.19 (the first real release, 2026-10-31) shipped HTTP/2 over TLS and cleartext h2c acceptance (0.5.12 to 0.5.14) and the `acme-tls/1` and HTTP-01 handlers (0.5.11 and earlier); 0.5.21 (client https) and 0.5.22 (the private-address guard) were published the same day**, all before this section existed. The PM acknowledged the miss on 2026-10-31 and asked for this re-sweep, bounded as below. This section records the order; it does not change it.

Run by: HTTPAnt lane, Claude Sonnet 5.5, one session, no subagents. **NOT legal advice; NOT a freedom-to-operate opinion.** Verdict words here are `outside as read` (no independent claim reads on what HTTPAnt does; the element that is missing is named) and never `clear`: `outside as read` means "I read the claims and the mechanism differs", not "cleared".

### Ideas swept in this pass (continuing the numbering above)

11. A TLS acceptor owned by the library, below interchangeable web libraries, that offers HTTP/2 by ALPN only for adapters that declare they can serve it, and accepts cleartext HTTP/2 with prior knowledge as a per-adapter measured capability.
12. Answering ACME validation inside the library: the HTTP-01 token path answered before routing, and TLS-ALPN-01 (`acme-tls/1`) answered in the TLS acceptor with a certificate from the pluggable certificate source.
13. (Post-first-sweep, not one of the four families the PM named, included because it is published and uncovered: queried with two searches.) Redirecting port 80 to 443 only for an allow-listed host and answering `421` otherwise; audit events for a refused handshake and a certificate change; reading back the certificate a listener really serves.
14. A client that verifies https against bundled roots with no switch to turn that off, proved by a conformance case that runs the client against test servers with an untrusted, a mis-named and a self-signed certificate, and by deliberately lenient verifiers (mutants) that must fail the case.
15. A private-address guard in an HTTP client at three points: filtering the name lookup's answers, refusing an IP-literal host before connecting, and checking the address the connection really reached before TLS and before any request byte.

### Queries (source, query, date)

All 2026-10-31. Source for every query: the lane's general web search limited to `patents.google.com` and `patents.justia.com`. **Espacenet, PatentScope and the USPTO's own interface were not searched in this pass** (the PM bounded it to four families by three queries; ppubs pages appeared incidentally in some results). Twelve queries.

| # | idea | query |
|---|---|---|
| C1 | 11 | patent HTTP/2 ALPN protocol negotiation TLS acceptor shared across multiple web server frameworks adapters |
| C2 | 11 | patent cleartext HTTP/2 prior knowledge connection preface detection server accepts without TLS |
| C3 | 11 | patent automated testing of HTTP/2 server implementations conformance suite protocol negotiation faulty implementation detection |
| C4 | 12 | patent TLS-ALPN-01 acme-tls/1 validation handshake certificate challenge response server |
| C5 | 12 | patent HTTP-01 challenge token well-known path web server automatic certificate issuance domain validation |
| C6 | 13 | patent redirect HTTP request on port 80 to HTTPS only for allowed host names otherwise reject 421 misdirected |
| C7 | 13 | patent TLS listener emits audit event on refused handshake and on certificate change; read back certificate actually served by listener |
| C8 | 14 | patent HTTP client verifies server certificate against built-in trust anchors, verification cannot be disabled, hostname mismatch rejected handshake |
| C9 | 14 | patent testing TLS client certificate validation using deliberately defective verifier accepts any certificate to show test fails |
| C10 | 15 | patent server-side request forgery prevention resolve hostname block private internal IP address before connecting outbound request |
| C11 | 15 | patent DNS rebinding protection pin resolved address filter DNS answer remove private addresses connect only to validated address |
| C12 | 15 | patent verify peer address of established connection against blocked network ranges before sending request outbound proxy internal network access prevention |

### Hits read (all independent claims, in full, with `tools/patent-claims.py`)

| # | patent | title | assignee | status (Google Patents' estimate) | claims read | overlap | our difference | verdict |
|---|---|---|---|---|---|---|---|---|
| H13 | US11770460B2 | Method and system for sharing multi-protocol port, and server | Wangsu Science and Technology | Active (est. 2041) | 1, 12 (independent), 2 to 11, 13 to 18 | A listener that detects the protocol of a connection; claims 8 and 9 mention HTTP/2 chosen in the SSL handshake | Claims 1 and 12 require an RTMP listening structure written into a designated field of an HTTP listening structure, the RTMP listener set to an ignore state, and detection on a port shared by the two protocols (an Nginx module). HTTPAnt has no second protocol on the port, no listening structures, and negotiates `h2` by ALPN in its own acceptor | outside as read |
| H14 | US10630758B2 | Method and system for fulfilling server push directives on an edge proxy | Radware | Active (est. 2036) | 1, 15 (independent), 2 to 14, 16 to 27 | HTTP/2 between an edge and a client | Both independent claims need a web accelerator outside a CDN issuing a PUSH resource designator to an edge proxy inside it; HTTPAnt has no CDN, accelerator or server push | outside as read |
| H15 | US10506080B2 | Communication apparatus, communication system, information processing method, and storage medium | Canon | Active (est. 2036) | 1, 6, 7, 8 (independent), 2 to 5, 9 to 17 | Starting HTTP/2 on a connection without the HTTP/1.1 upgrade; claim 11 sends the HTTP/2 connection preface | A **client** that stores per-destination HTTP/2 setting information and skips the HTTP/1.1 exchange only if it has the settings stored for that destination. The HTTPAnt client speaks HTTP/1.1 only and keeps no such store; the server side merely accepts a preface (and on some adapters declines it) | outside as read |
| H16 | US7970939B1 | Methods and systems for addressing DNS rebinding | Symantec / CA | Active (est. 2029) | 1, 12, 15 (independent), 2 to 11, 13, 14, 16 | Defending against a name that changes its address | Binds a domain to the first address it resolved to, stores a domain-name record and prohibits changing it until a request to leave the domain is detected (browser plug-in or kernel DNS proxy). HTTPAnt keeps no record, has no notion of leaving a domain, and removes private addresses from each lookup's answer | outside as read |
| H17 | WO2014043228A1 | Domain name system rebinding attack protection | an individual | Ceased | 1, 16, 21 (independent), 2 to 15, 17 to 20, 22 to 30 | Same family of problem | Compares a subsequent DNS request's answer with an earlier answer for the same name (and the zone of each) and takes a protective action; HTTPAnt compares no two lookups | outside as read |
| H18 | US8776238B2 | Verifying certificate use | IBM | Expired, fee (Google Patents lists est. 2031) | 1, 6, 11, 16, 21 (independent), 2 to 5, 7 to 10, 12 to 15, 17 to 20 | A test peer sends an invalid certificate to see whether the application continues the session, to find applications that do not verify; this is the closest in idea to case 14's test servers | Every independent claim needs a certificate use verification application working with a **proxy server** that intercepts a *second* message and performs correct certificate processing for it. The HTTPAnt suite is a test server and a client under test: no proxy, no interception, no correct processing on the client's behalf | outside as read |
| H19 | US7797590B2 | Consensus testing of electronic system | Codenomicon / Black Duck | Active (est. 2029) | 1, 20, 21, 22 (independent), 2 to 19 | A conformance-style test of protocol implementations | Generates a verdict by comparing elements of traffic from tests of **several systems** and voting on the consensus. HTTPAnt's cases check each implementation against a stated requirement over real sockets, and prove the case can fail with a deliberately broken one; no comparison across systems | outside as read |
| H20 | US20210266185A1 | Home or Enterprise Router-Based Secure Domain Name Services | McAfee | Active (est. 2041) | 1, 15, 19 (independent), 2 to 14, 16 to 18, 20 | Mentions an ACME client getting a certificate (claim 13) | A home router providing DoH or DoT with a provisioned certificate, rules per domain name; claim 13 is a dependent ACME client, which HTTPAnt does not contain (it answers challenges, it does not run an ACME client) | outside as read |
| H21 | EP2154817A1 | Server certificate issuing system | GlobalSign | not stated in the page read | 1 (independent) and the dependent claims | A web server publishing a verification page that a registration server reads to confirm control (the idea behind HTTP-01) | A system of a registration server and an issuing authority generating a key pair, a CSR and the certificate, issuing only if the page is verified. HTTPAnt serves the token a certificate authority asks for through a handler the caller supplies; it issues nothing and generates no CSR | outside as read |

No hit needs review. **Nothing in this pass reads inside.**

### Seen in the results, not read

US10110642 (Canon, the sibling of H15; same family of idea, not read), US5958052 (DNS proxy filtering private information, 1990s filing; not read), US20170063778A1 (an attacker technique using DNS rebinding), US9300687 (restricting private ranges, rebinding caveat), US6003084 (secure network proxy with an access control list), US10171452B2 and US9565180B2 (trusted-CA lists in clients), US20220046024A1 (a gateway pinning certificates). Listed so a reader can see what the searches returned; none was read, so no verdict is given on them.

### What this pass did not find, and what that is worth

No patent surfaced for: server-side detection of the cleartext HTTP/2 preface; `acme-tls/1` validation in a TLS handshake; the `421` host allow-list redirect; a listener reading back its own served certificate; a check of the connected peer address before TLS. **An absence of search hits is weak evidence.** Twelve queries through a general web search limited to two patent sites, each returning about ten pages, cannot show absence; they show only that nothing close came back.

## Limitations

AI-assisted due-diligence read; not legal advice; not a freedom-to-operate opinion. Ten queries through a general web search that returns a handful of Google Patents and Justia pages each, so coverage of the patent literature is thin and an absence of hits is weak evidence. Seven patents had their claims read; the machine-converted claim text should be checked against the official record before any quote is relied on. Status and expiry are Google Patents' estimates. The first two passes describe the code at `2d8a670`/`340e667`; the re-sweep section above covers HTTP/2, the ACME handlers, client https and the private-address guard at `6d564a0` with twelve more queries and nine more patents read. Still outside every pass: adapters for libraries other than the six in tree, client slice 3 (OS root store, proxy, opt-in redirects, not built), and any classification-code search or search by a person with the patent offices' own tools.
