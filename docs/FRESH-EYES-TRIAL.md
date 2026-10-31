# Fresh-eyes trial of the published-shape package

EXPECTATIONS 6.24: *a fresh-eyes trial (an agent given only the README must reach the example's result) is part of the first-release gate.* This is
the record of the first one. It is advice for the person deciding the publish, as well as a required step: a pass that reads as lucky, or an ugly
path it took, is a reason to hold.

**Run 1, 2026-10-31**, a fresh `sonnet` subagent (no memory of this project), Windows, offline, at `feat/release-packaging` (0.5.18 placeholder).
**Result: pass**, and the README was fixed from it; **run 2** and **run 3** (below), each with a new agent, also passed. All three tasks worked on the first attempt from the README alone, with no compiler error or warning. It took the agent about
60 seconds of work; about 25 s of that was the first compile.

## What the agent was given, and what it was not

- The README of the **packaged** `httpant` crate (byte-identical to the repository README, checked with `cmp`), and nothing else about HTTPAnt.
- A cargo configuration that replaces crates.io with a **directory source** holding the 40 crates a published `httpant` would pull in (vendored
  from a seed project that depends on it) **plus the four crates exactly as `cargo package` made them** (`httpant`, `httpant-core`,
  `httpant-server`, `httpant-client`, extracted from their `.crate` tarballs, with their checksums). So `cargo add httpant` resolves to the
  packaged 0.5.18 and the build compiles the packaged sources, not the checkout.
- Tasks: (1) from the README alone, create a binary project and get the first example running; (2) switch to the second example's
  `Server::with_adapter("hyper")?`; (3) find out what an unknown id does.
- Rules it was given: use only the README, do not read other folders or the vendored sources, do not search the web.

## Limits of the method (so the pass is not read as more than it is)

- **The agent was told, not prevented,** from reading other folders. Its report lists five tool calls and none of them looks outside the working
  directory, but nothing enforced it.
- **The mirror simulates publication.** A real stranger's `cargo add httpant` reads the crates.io index and downloads; here it reads a directory.
  What it cannot show: the crates.io page rendering, the docs.rs build, and how long a cold download takes.
- **One agent, one run, one platform.** An AI model is a proxy for a stranger, not a stranger; it has Rust knowledge a newcomer to Rust would not.
  It does show that nothing in the README or the package needs knowledge that is not written down.
- The `cargo add` output listed every optional feature (`actix-web`, `axum`, `http2`, `poem`, `salvo`, `tls`, `warp`), which is good, and means
  the agent could see the adapters exist without reading anything else.

## The agent's report (model output, quoted)

> **1. Commands, in order** (all in `...\scratchpad\trial\stranger`)
> 1. `cargo new try1`, which succeeded.
> 2. `cd try1 && cargo add httpant`, exit 0. It added httpant v0.5.18 and listed these optional features: actix-web, axum, http2, poem, salvo, tls, warp.
> 3. `cargo add tokio --features macros,rt-multi-thread`, exit 0. It added tokio v1.54.1.
> 4. I pasted the README's first example into `src/main.rs`, then ran `cargo run`. **Exit status 0.** The last lines were:
> ```
>    Compiling httpant v0.5.18
>    Compiling try1 v0.1.0 (...\stranger\try1)
>     Finished `dev` profile [unoptimized + debuginfo] target(s) in 25.83s
>      Running `target\debug\try1.exe`
> ```
> The example prints nothing, so a silent run with status 0 is the success signal.
> 5. I changed `Server::new()` to `Server::with_adapter("hyper")?` and added a println and the "nonexistent" check. `cargo run` again gave **exit status 0**:
> ```
>     Finished `dev` profile ... in 1.86s
>      Running `target\debug\try1.exe`
> ok: hello
> error: unknown adapter `nonexistent`: use one of ["hyper"]
> ```
>
> **2. Errors and warnings.** None, so nothing needed resolving.
>
> **3. Results.** Task 1: yes, 1 attempt. Task 2: yes, 1 attempt. Task 3: yes, 1 attempt (`Server::with_adapter("nonexistent")` returns an `Err`; matched
> on it and printed it with `{}`; the compiler needed no help, because `httpant::Error` implements `Display`).
>
> **4. README issues.**
> - **Mismatch on the error message.** The README says an unknown id "is an error that lists the ids you can use and the feature that enables each."
>   The actual message was ``unknown adapter `nonexistent`: use one of ["hyper"]``. It lists the usable ids, but it names no feature. With only default
>   features that may be right, since hyper needs none. A stranger would still not learn from the message that `axum` and the others exist, or which
>   feature turns them on. The README's own comment line does say that.
> - **Silent success.** The first example prints nothing, so a newcomer cannot tell it ran. The README could say "it prints nothing; exit code 0 is success".
> - **Second example is a fragment.** It only shows `with_adapter("hyper")?` inside a different program that ends in `server.shutdown().await`. I had to
>   work out for myself how to merge it with the first example. It worked, but the README never says the rest of the first example stays unchanged.
> - **Task 3 has no worked example.** The README never shows how to print an error. I used a `match` and `{}` from general Rust knowledge, and the
>   README's error sentence does not say the error implements `Display`.
> - **Bare `Result`.** `httpant::Result<()>` is used without explanation. It compiled fine.
> - **Minor.** The Install section comes after the first example, so a stranger copies code before seeing the setup commands.
>
> **5. Verdict.** Yes, a stranger can get from the README to a working program without help. It took about five minutes, most of it the first compile,
> and the cargo add output was clear. The single biggest friction was the gap between the README's claim about the unknown-id error and the real
> message, which lists only `hyper` and names no features. Close behind was the lack of any visible output from the first example.

(The agent's "about five minutes" is its own estimate; the harness measured about 60 s of agent work, most of it the first compile.)

## What was done about each finding

| finding | verdict | what changed |
|---|---|---|
| README claimed one error lists the ids *and* the feature | **right, and a wrong claim of ours.** There are two errors: `unknown adapter ...: use one of [...]` (an id that does not exist) and `adapter ... is not compiled in: enable the ... feature` (an id that exists but whose feature is off) | README now quotes both messages exactly and says which mistake gets which; both are covered by unit tests in `httpant-server` |
| the first example prints nothing | right | README says it prints nothing and that exit status 0 is the success signal |
| the second example is a fragment | right | it is now the first program with the one call changed, complete and compiled as a doctest, and the text says so |
| no way shown to print an error | right | README says `httpant::Error` prints with `{}` as a sentence ending with what to do next |
| bare `Result` | right | README says `httpant::Result<T>` is `Result<T, httpant::Error>` |
| Install after the example | **kept on purpose:** the rule (6.24) is that the hello-world comes first | a line under the example now points at the two `cargo add` commands |

The README changed after the trial in response to it, in wording only: the first example is unchanged byte for byte (and still checked against
`examples/hello.rs` and the hub card by a test), and the second now compiles as a complete program. The PM asked for a re-run on the revised
package; it follows.

## Run 2: the revised README, a new agent

**Run 2026-10-31**, a second fresh `sonnet` subagent (it was not told what the first found), on the crates re-packaged from the revised tree
(`feat/release-packaging`, 0.5.18 placeholder; the mirror was rebuilt, the README `cmp`-identical to the repository's). Same tasks, with task 3
widened to both errors. **Result: pass.** All three tasks first attempt, no error or warning, about 66 s of agent work.

- Task 1 and 2: `cargo add httpant` (added 0.5.18), `cargo add tokio ...`, the README's example in `src/main.rs`, `cargo run` exit 0; then
  `Server::new()` became `Server::with_adapter("hyper")?`, `cargo run` exit 0. It noted that "the program printed nothing, as the README says".
- Task 3 output, as the agent saw it: ``nonexistent: unknown adapter `nonexistent`: use one of ["hyper"]`` and
  ``axum: adapter `axum` is not compiled in: enable the `axum` feature``. Its words: "Both messages match the README's quoted text character for character."
- Its verdict (quoted): "yes, a stranger could get from the README to a working program without help. The README's instructions worked as written, and
  every message I saw matched what the README said. The single biggest friction was minor: the Install section is below the first example, and the
  README never shows the error-handling snippet for Task 3."

What it raised, and what was done:

| finding | verdict | what changed |
|---|---|---|
| no snippet for catching and printing the error | right | a short `match` example, compiled and run as a doctest (the README now has three doctests) |
| "Read them from `Origin`" is confusing; it could not tell what `Origin` is | right | the bullet now says a handler reads `httpant::server::Origin` from the request's extensions, and what it holds (client address, scheme, whether it came through a trusted proxy) |
| Install is below the first example | kept on purpose (rule 6.24: the hello-world first); the sentence under the example points at it, and the agent noted that sentence | none |
| it did not test the "id from your settings" claim, nor the feature-off message with the feature on | not a defect; the first is a `&str` argument, the second is covered by unit tests in `httpant-server` | none |

These two wording changes (a sentence, and a doctested snippet) came after run 2; run 3 covers them.

## Run 3: the final packaged form, a third agent

**Run 2026-10-31**, a third fresh `sonnet` subagent, on the crates re-packaged after the packaging branch was rebased onto `main` (restructure #37
merged; 0.5.18 placeholder; README `cmp`-identical to the repository's; mirror refreshed). Same tasks as run 2. **Result: pass.** All three tasks first
attempt, exit status 0 each time, no error or warning; the cold first compile took 1 m 44 s. Both messages matched the README character for character.

Its words (quoted): "Yes, a stranger can get from README to a working program without help. Install, the first example, the adapter switch and both error
messages all worked as written, first try, with no warnings. The single biggest friction was minor: the first example is silent ... and the cold build
takes about 1m45s." Smaller notes it raised, none acted on: the README does not show a full axum example (it says `cargo add httpant --features axum`;
the agent did not try that path); it does not state the minimum Rust version in words (it points at `rust-version` in `Cargo.toml`, which the packaged
crate does contain); it does not mention the cold build time. Not defects; recorded here so the PM can decide whether any is worth a line.

## To repeat it

1. `cargo package --workspace --locked --allow-dirty --no-verify` (the `.crate` files appear in `target/package/`).
2. Make a seed project that depends on `httpant` (by path) and `tokio`, and `cargo vendor` it into a `vendor/` directory; extract the four `.crate`
   files into `vendor/`, each with a `.cargo-checksum.json` of `{"files":{},"package":"<sha256 of the .crate>"}`.
3. In an empty directory put the packaged `README.md` and a `.cargo/config.toml` that replaces `crates-io` with that directory and sets
   `net.offline = true`.
4. Give a fresh agent (on Sonnet, never Opus) only that directory and the three tasks above.
