# Vendored crates

Two GPUI backend crates, each from crates.io with one patch applied. The root `Cargo.toml` points
`[patch.crates-io]` at them. Only `gpui-pre-platform` depends on them, per target, so each compiles on
its own OS only. The design is in `docs/design/linux-cef/design.md` section 2.2.

| Crate | Version | Patch | What the patch does | Without it |
|---|---|---|---|---|
| `gpui-pre-linux` | 0.3.9 | `gpui-pre-linux.patch` (`src/linux/x11/client.rs`) | adds a 1×1 InputOnly `gpui-focus-proxy` child per toplevel, whose key events count as the toplevel's. `FocusIn`/`FocusOut` with detail `Inferior` leave the window active and reset only compose, pre-edit and IME state | keys typed in the omnibox go to the page under the pointer, and GPUI turns inactive on every page click (spike-core.md section 7) |
| `gpui-pre-windows` | 0.3.9 | `gpui-pre-windows.patch` (`src/platform.rs`, `translate_accelerator`) | sends `WM_GPUI_KEYDOWN` only to windows of class `Zed::Window` | every key posted to a page is dropped (spike-win.md section 4) |

Both are plain bugs for any embedder of foreign child windows. Each gets a Zed PR in wave 1, linked from
`docs/design/linux-cef/log.tsv`. Once a fix ships in a gpui-pre release, the next bump deletes that
crate's `[patch]` line, its tree, its patch and its `vendor.toml` entry in one commit.

## Rules

- Nobody edits a tree by hand. `cargo xtask vendor-gpui --bump <crate> <version>` reads the `.crate`'s
  checksum from the crates.io sparse index, downloads and verifies it, unpacks it into
  `third_party/<crate>/`, applies `<crate>.patch` and records both digests in `vendor.toml`. It is the
  only step that trusts the network; review its `vendor.toml` diff.
- `cargo xtask vendor-gpui --check` (CI) downloads each `.crate`, checks it against `crate_sha256`,
  checks the patch against `patch_sha256`, applies the patch in memory and fails if the result differs
  from the committed tree in any byte or file.
- Patches are unified diffs with `a/` and `b/` paths relative to the crate root. They apply strictly:
  every hunk at its stated line, with no offset and no fuzz.
- `third_party/** -text` in `.gitattributes` keeps the trees byte for byte on a Windows checkout.

## Bumping GPUI

1. Change the `gpui`, `gpui_platform` pins in the root `Cargo.toml`.
2. `cargo xtask vendor-gpui --bump gpui-pre-linux <version>` and the same for `gpui-pre-windows`. If a
   patch no longer applies, fix it against the new crate and rerun.
3. Rerun the checks of the design's section 7 on both OSes.

The patches started as `docs/design/linux-cef/spikes/core/vendor/gpui-x11-focus.patch` and
`docs/design/linux-cef/spikes/win-core/vendor/gpui-windows-keys.patch`, with the Linux one's paths
rewritten to the `a/`, `b/` form. The trees they produce are byte-identical to the spikes' vendored
`client.rs` and `platform.rs`.
