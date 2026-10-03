# PWA persisted display context

CORE-PWA04 adds an opt-in raw display context to workspace operations and `reconcile.rebase.v3`. The existing contracts remain unchanged when this field is absent. This capability is not present in the official Core 0.43.0 artifact.

## Request schema

The optional field is named `displayContext`. Its only profile is `pwaStorage`. The object has exactly two required fields:

```json
{
	"profile": "pwaStorage",
	"projectionPending": {
		"commands": [],
		"taskOperations": [],
		"durationOperations": [],
		"autoStartOperations": [],
		"selectedTaskOperations": []
	}
}
```

`projectionPending` contains the actual persisted record payloads, including extensions and omitted or null fields. All five arrays are required. Each array can contain a subset of its complete retained queue.

An explicit `projectionPending: null` represents a missing or null legacy metadata record. It selects the original PWA fallback, which displays all retained records. A null `displayContext` object is invalid. Omitting `displayContext` keeps the existing delivery-safe workspace contract.

The field occurs at these locations:

- `workspace.project.v1`: request root.
- `workspace.readModel.v1`: `source.value`, with outer `profile: "pwaStorage"`.
- `workspace.intent.v1`: `workspace`, with `compatibility: "pwaStorage"`.
- `workspace.completionMutation.v1`: `workspace`, with `compatibility: "pwaStorage"`.
- `reconcile.rebase.v3`: request root.

Other read-model or mutation profiles reject the field. V1 and V2 reconciliation do not consume this capability. The existing bootstrap contract continues to accept its raw record at `local.projectionPending`. A workspace-level context is invalid in a bootstrap request.

The canonical base, complete retained queues, covering head, delivery proof, and dependency records remain required by their existing contracts. The context does not replace any of these inputs. No eligibility flag, send permission, synthesized proof, or alternate canonical base enters this schema.

## Validation and display selection

Core validates the complete retained ledger before it filters display records. The existing validators check wire values, clocks, device sequences, dependencies, canonical timer provenance, task identities, and preference values. Mutation entrypoints retain their existing ownership checks.

Every stored record must exactly equal a current retained record in the same domain. Unknown identities, stale removed identities, duplicate identities, rewritten fields, added or removed extensions, incomplete arrays, duplicate JSON keys, and unknown context controls fail closed. An exact extension on both records remains valid. Typed equivalence is insufficient.

`reconciliation/workspace/display.rs` owns the selection and membership validator. Bootstrap uses the same functions. The selection preserves the original `server/web/app-state.js` policy at server commit `50c86a2`:

- Existing matching stored arrays remain eligible for display after proof retirement, even when the head covers their records.
- Core adds an undisplayed retained command only when it has durable never-sent proof and its clock exceeds the head. A null head permits this command addition.
- Fresh command selection is per record. One old or claimed command does not suppress a newer proven command in the display context.
- Read operations do not add fresh task or preference records to a non-null stored context.
- A Core-planned task or preference mutation updates the affected display domain only when the existing whole-domain delivery policy accepts that domain. Claimed or stale domains retain their queued outcome. This preserves existing task and preference admission.

The timer reducer owns the resulting timer and history. A read model exposes controls for that timer. Intent targeting and completion use the same display selection. Retained-ledger admission remains a separate check.

## Results and persistence

`workspace.project.v1` keeps `projectionPending` as its delivery-safe result. With a context, `workspace` is the display result and a separate root `displayContext` contains the Core-derived display records. These two queue sets can differ. In particular, a head-covered claimed Start can display a running timer while `projectionPending.commands` remains empty.

Intent and completion return the updated context at `workspace.displayContext`. A null-head fresh Start receives an applied display outcome. Its returned context contains the new command, while the delivery-safe projector still suppresses the null-head queue. The intent does not create a covering head or retire proof.

The host persistence contract commits the returned context atomically with the returned complete queues, proof, dependencies, allocation, observations, selection, and ownership writes. A failed transaction persists none of the group. The canonical snapshot stays unchanged by optimistic replay. Reads remain pure.

V3 validates the incoming context against the complete pre-ACK ledger. After ACK processing, dropping, and authorized generated-break normalization, Core resolves its stored identities against the remaining raw pending records. Removed records disappear from the returned context. Authorized rewrites use the new Core-derived payload. Possibly delivered records preserve their exact original payloads.

V3 returns the context at root `displayContext`. Its `projectionPending` remains delivery-safe, and its `workspace`, `timer`, and other projection fields contain the selected display result. `canonicalResponse` and the canonical base fields retain the actual server evidence. Empty display records do not create a timer from history. A server-provided terminal timer remains authoritative.

Context records are projection inputs, not replacement retry payloads. JSON whitespace and key order are not byte-preserved. The host retains the original queued records and saved request strings for exact delivery retries.

Core cannot prove that supplied records came from persistence, that the host supplied every row, or that the base and head belong together. Core also cannot prove historical delivery or atomic host persistence. Those remain host preconditions. Exact membership prevents a context from inventing or rewriting a retained record, but it does not authenticate storage.

## Evidence

`fixtures/pwa-display-context-v1.json` preserves both complete persisted observations from the independent migration checker. The covering-head observation includes the raw saved request body, captured sent arrays, retired proof, canonical snapshot, display record, owner, allocation, and observation metadata. The null-head observation preserves its original empty stored context and proven pending Start.

`scripts/pwa_display_context_probe.mjs` compares these fixtures with `pwa-checker-evidence.json`. It executes the preserved production source's display selector and compares the selected queues with native Core. It also checks the existing official artifact's digest and size, proves its idle baseline and unknown-field failures, and compares bootstrap validation with the shared policy.

The probe saves full raw requests, decoded requests, complete native envelopes, official failure envelopes, persisted inputs, and the PWA lifecycle corpus. It does not build WASM. Its arguments are the official WASM path, checker evidence path, server repository path, and output path:

```sh
node scripts/pwa_display_context_probe.mjs \
	../server/web/pomodorough_core.wasm \
	/path/to/pwa-checker-evidence.json \
	../server \
	/path/to/core-pwa04-proof.json
```

Native Rust tests verify both genuine observations, absent-context compatibility, exact extensions, stale records, and suppressed malformed preferences. The aggregate corpus covers fresh Start, claim selection, proof retirement, a covering head, serialized reload, Pause, Resume, manual and automatic Finish, generated Start, direct child dependencies, V3 ACK promotion and rejection drops, ownership denial, and no synthetic timer after removal. The head and proof matrix covers null, lower, equal, and higher heads with absent, partial, and complete proof. Task and preference cases compare complete production returns after removal of the new context field.

The aggregate gate also asserts semantic branch counts and kills output mutants. `tests/aggregate_wasm_parity.mjs` transfers the same raw inputs and checks complete raw envelope parity against the exact hosted artifact.

## Official release requirements

Local verification uses pinned Rust 1.97.1 for formatting, warning-denying Clippy, and native tests. Exactly these two tests are skipped because they build WASM locally:

- `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host`
- `c4_release_wasm_rejects_oversized_allocations_without_trapping`

The release workflow must run all Rust tests, build and canonicalize WASM on its pinned hosted producer, verify the artifact, and execute every `tests/*.mjs` gate against those exact bytes. The aggregate gate must include the PWA branch counts and raw-envelope comparisons. The tested bytes must pass the existing checksum, immutable candidate, and attestation gates before publication.

A release consumer must verify the published digest and attestation before adoption. Client integration must forward actual raw metadata and commit Core returns atomically. This Core-only change does not establish client adoption, a passing complete PWA suite, or hosted WASM parity.
