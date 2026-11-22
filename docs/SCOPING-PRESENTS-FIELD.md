# Scoping note: the `presents` field of the capability graph, beyond claude-code

Docs only, by the httpant lane for pm-ants' [TASK] 2026-11-22 (free-capacity rule, capant's rank 3). **Read, not run.** The graph is PM-owned and nothing here edits it. Sources read on 2026-11-22: `C:\Projects\CAPABILITY-GRAPH.json` (21,149 bytes, the shared tree, no commit to cite), `C:\Projects\tools\capability_graph.py` (lines 58 to 60 and 75 to 77), `C:\Projects\AGENT-STACK.md` line 64, and capant's `docs/AGENT-STACK-GAPS.md` at `origin/stack-gaps-answers` `a77d63d` (the "Presents vs implements" row and the "Owner check" section). Nothing here is decided.

## What exists today

- One project has the field: `claude-code` has `"presents": ["run-model", "sampling"]` with the note that the model is Anthropic's, reached over the network (graph, the `claude-code` entry). No other project has one.
- A `presents` entry is a bare capability id, with no status, no evidence and no pointer to who implements it. The checker validates only that the id exists (`capability_graph.py:58-60`); the matrix prints a `^` ("presented but implemented elsewhere", `:75-77` and the legend); `gaps()` ignores `presents` entirely.
- The idea comes from AGENT-STACK.md line 64 (CireSnave's point, PM wording, undecided): a product can present a layer without implementing it, and an equivalent of Claude Code must present layer A (model identity, context size) upward through the harness adapter.

## What the field cannot say today (the scoping questions)

1. **Where is "elsewhere"?** `^` says a capability is implemented somewhere else but not where. For `claude-code` the implementer is Anthropic's model, which is not a graph project, so even the graph's `run-model` row (lightbulb, ollama, both `claimed`) does not point at it. A reader of the matrix cannot tell a presented capability with a real provider from one with none.
2. **Who may write it, and what makes it true?** The README says a lane confirms its own `provides` rows with a status and evidence. `presents` has neither, so a lane could add one and no check could tell whether the claim is right. The graph's own rule (EVERY link is a PM claim until a lane confirms it) does not reach this field.
3. **Does it count for gaps?** A presented capability with no confirmed provider anywhere is invisible to `gaps()`: `claude-code` presenting `sampling` makes `sampling` look no different from one nobody touches. Whether presenting should close, narrow or leave a gap open is a PM decision.

## Who could use it beyond claude-code (a reading of the graph, not a probe)

- Of the projects in the graph only `claude-code` and `ollama` have `kind: product`. `ollama` implements (`run-model` and `serve-openai-api`, both `claimed`), so it is not a case of presenting.
- No project of ours was found that presents a capability it does not implement; I did not look inside their code, so that is "none found in the graph", not "none".
- The one foreseeable user is the agent-loop's harness, which is not a graph project yet (the `projects` list has no `agent-loop` entry). Per capant's notes it adopts `rig` as its engine, and AGENT-STACK line 64 says an equivalent must present A upward; if that is built, it presents `run-model` and `sampling` the way Claude Code does, over a network model.

## Recommendation (the PM decides)

1. Add no `presents` entry to any other project until `agent-loop` is a graph project: today there is no second user, and a field with one user cannot show whether its shape is right.
2. When it is added, give an entry an optional pointer to the implementer (for example `["run-model", "via": "anthropic-api"]`, or a note convention) so `^` can say where; this changes the shape of the entries and `capability_graph.py:58` (it iterates the entries as ids), so the change is the PM's and the checker's, not a lane's.
3. Decide separately whether `gaps()` should list a presented capability that has no `built` or `partial` provider as a gap with the note "presented by X".

Not done: no check of whether a second presenting product exists outside the graph; no read of agent-loop's repo beyond what capant's note quotes.

— lane: httpant
