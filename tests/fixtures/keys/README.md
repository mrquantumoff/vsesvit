# Test-only signing keys

These RSA-2048 private keys are committed on purpose and are therefore public. They exist
only to sign CRX3 files in tests and in the shells' `--self-test`. Never use them to sign
anything that ships.

| file | used for | extension id it derives to |
|---|---|---|
| `test-only-probe-key.pem` | `vsesvit_core::testkit::probe_crx()` | `eonajgebgeenbhiiobbhmkafolkeghdb` (`testkit::PROBE_ID`) |
| `test-only-second-key.pem` | second proofs and wrong-key cases in `tests/extensions_crx.rs` | `kpjcccehndkfpfbbofnjkcelnoonmhja` |

Both are PKCS#8 PEM with a one-paragraph label before the `-----BEGIN` line; `testkit`
skips the label when it parses them.
