Everything is done and green on both platforms. Stopping the now-redundant watchers is handled automatically when I finish, and no process I started is still running.

**Findings**

- **[0] FIXED** — tests `a_record_with_two_transitions_at_one_visit_key_is_stored_as_one_visit_and_repaired`, `copied_profiles_visiting_in_the_same_millisecond_still_merge` (tests/core_history.rs), `history::tests::normalize_keeps_one_visit_per_key_with_the_greatest_transition` (unit), `page_records_converge_through_the_table` (tests/convergence.rs, new proptest). Before the fix the first two failed with `UNIQUE constraint failed: history_visits.url, history_visits.at_ms, history_visits.device` and the batch (including a Prefs record) rolled back. Fix: `history::normalize` now collapses visits to one per `(at_ms, device)` (the table key) keeping the greatest transition, before the cap; the in-memory join stays a plain union so `lattice_laws` still holds, and the stored/exported form is a deterministic function of the union (merged != incoming marks the canonical form dirty, so a peer's doubled record is repaired on the server and then applies as a no-op).
- **[1] FIXED** — tests `json_registers_carry_a_null_value_distinct_from_no_value` (tests/core_wire.rs), `a_null_value_is_a_value_on_every_device_and_never_echoes` (tests/core_ext_storage.rs), `a_pref_whose_value_is_json_null_syncs_as_a_value` (tests/core_prefs.rs), and `devices_converge` now generates JSON null storage values (before the fix it failed with "no quiescence after 4 rounds: records are echoing"). Fix: new `crdt::json_register` serde module used by `PrefRecord.value` and `SyncItemRecord.value`; an `Lww<Option<JsonText>>` now travels as `{"v": "<canonical json text>" | null, "at": stamp}` (the two states of the DB column), so `Some(null)` and `None` are distinct while `Extra` keeps the value form. Wire format change for those two kinds only; the `{"v","at"}` rule is kept (exactly two keys, `v` present, `at` a stamp; a non-JSON `v` is rejected at the boundary). No statement in docs/design/core.md became false (it names the type `Lww<Option<JsonText>>` and the `{"v","at"}` shape, neither the value encoding), so the doc is unchanged.

Observation (not one of my findings, not changed): the new proptest initially generated a page record with zero visits plus an `extra` field; `settle` drops such a record (page existence is derived from visits), so within a single batch the outcome depends on order. A device never exports a visit-less page and the dirty rule repairs it on the next exchange (which is why `devices_converge` settles), so I constrained that test to exportable records and left the semantics alone.

**Files changed** (all under `C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\fix-core\crates\vsesvit-core\`)
- src/crdt.rs (json_register module, JsonText doc)
- src/history.rs (normalize dedupe + docs, unit test)
- src/prefs.rs, src/ext_storage.rs (`#[serde(with = "crate::crdt::json_register")]`)
- tests/core_history.rs, tests/core_wire.rs (new test; `"dark"` → `"\"dark\""` in `other_records_follow_the_rule`), tests/core_ext_storage.rs, tests/core_prefs.rs, tests/convergence.rs (null values in `StorageSet` and `json_value()`, new `page_records_converge_through_the_table`)

**Output tails**
```
Windows  cargo test --workspace: all "test result: ok" (core: 33 unit; convergence 4 passed; core_history 6; core_ext_storage 4; core_prefs 5; core_wire 10; ...) — exit 0
Windows  cargo clippy --workspace --all-targets: Finished `dev` profile ... (no warnings)
WSL      cargo clippy --workspace --all-targets: Finished ... exit 0 (no warnings)
WSL      cargo test --workspace: 33 "test result: ok", 0 failed — exit 0 (convergence: 4 passed in 9.41s)
Windows  self-test: [selftest] self-test: ok=true ... windows ok, 15/15 checks ok — exit 0
Linux    self-test: [self-test] PASS: 14/14 checks passed; report at /home/dy/st-fix-core/report.json — exit 0
WSL      PROPTEST_CASES=1000 devices_converge: test devices_converge ... ok — test result: ok. 1 passed; finished in 24.17s
Windows  PROPTEST_CASES=1000 devices_converge: test devices_converge ... ok — test result: ok. 1 passed; finished in 101.40s
```