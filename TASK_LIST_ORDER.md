# Task list order

Core builds every task list in one order (PWA-16). Before this change the list
was sorted by the raw UTF-8 bytes of the title, so "Bulk task 10" came before
"Bulk task 2" and every capital letter came before every small letter.

## Which lists Core orders

Core orders the task lists that its task reducer builds:

- `tasks` in the output of `task.reduce.v1` and `projection.apply.v2`.
- `tasks` and `workspace.tasks` in the output of `workspace.project.v1` and of
  `reconcile.rebase.v1`, `reconcile.rebase.v2` and `reconcile.rebase.v3`.
- The projected task list inside every other operation that runs the same
  projection.

Core does not reorder a task list that it copies from its input. Such a list
keeps the order in which the caller or the server sent it:

- `baseTasks` in the output of the three `reconcile.rebase` operations. It is
  the `tasks` array of the server response, unchanged.
- `canonicalResponse.tasks` in the output of `reconcile.rebase.v3`. It is the
  parsed server response, unchanged.
- A `base.tasks` array or a stored workspace that an operation echoes.

These lists are in the order below only when the Core that built them already
used it. Core does not check the order of a task list it receives.

The legacy fixture operation `projection.reduce` lists tasks by ID.

Wire shapes, operation names and task identity do not change; only the
position of tasks in the arrays that Core builds does.

## Rules

Two titles compare as sequences of tokens, left to right. The first different
token decides. A title that is a prefix of another lists first.

1. A run of ASCII digits (`0` to `9`) is one token and compares by numeric
   value. Leading zeros do not change the value. Runs of any length compare
   without overflow: the run with more significant digits is larger, and runs
   of the same length compare digit by digit.
2. Every other character compares by its case-folded code points. The fold of a
   character is `to_lowercase(to_uppercase(c))` from the Rust standard library
   of the pinned toolchain. It uses no locale. "ß" folds to "ss" and "ς" folds
   to "σ". One character is an exception: "ẞ" (U+1E9E, capital sharp s) folds
   like "ß", to "ss". The standard library leaves it out of `to_uppercase`, so
   the plain fold would stop at "ß". With this exception the fold is stable:
   folding folded text changes nothing.
3. A digit run ranks where the character `0` would. Digits therefore list after
   a space and most punctuation, and before letters.
4. Titles with equal tokens differ only in leading zeros or in characters that
   fold to the same text. Case is one such difference; "ß" and "ss", "ı" and
   "i", and "ﬃ" and "ffi" are others. The raw UTF-8 bytes of the title then
   decide, so "Apple" lists before "apple", "Task 007" before "Task 7" and
   "ss" before "ß".
5. Two tasks with the same title compare by ID. Task identity hashes the
   normalized title, so this happens only for input that did not come from
   `task.identity.v1`.

The result is a total order: two titles compare equal only when they are the
same string, and the list does not depend on the order of the operations.

## Limits

- Only ASCII digits are numbers. Fullwidth and other Unicode digits compare as
  text, one code point at a time.
- There is no locale collation. Accented letters list after "z", and scripts
  list by code point.
- There is no Turkish rule. "ı" folds to "i" and lists with "I" and "i". "İ"
  folds to "i" and a combining dot (U+0307) and lists after "iz".
- ASCII punctuation lists by code point around the digits and letters. A digit
  run lists after "/" and before ":". Letters rank as small letters, so they
  list after "_" and "`" and before "{".
- The fold tables belong to the Rust toolchain. A toolchain update that changes
  Unicode case mappings can move a title; `tests/task_list_order_wasm.mjs`
  checks that the release artifact and native Core agree.

## Clients

Clients must render the list in the order Core returns. A client that sorts
tasks by title itself must not use bytewise or locale order for the task list;
it has no reason to sort at all.
