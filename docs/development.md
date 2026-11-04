# Development

Rust is pinned to 1.99.1 in `rust-toolchain.toml`, edition 2024, resolver 3. `Cargo.lock` is
committed and every check runs with `--locked`; nothing refreshes dependencies silently. The
core uses only std; `keyrill-ffi` adds `libc` on macOS for file-open constants. Proptest and
serde_json are test-only. Don't add an async runtime, a UI framework or a second engine.

On macOS you also need Command Line Tools and clang-format 23.1.3 (`pip install
clang-format==23.1.3` in a virtual environment). Xcode is not required. Run the checks as an
ordinary user: one test depends on a file-permission denial.

```console
$ ./tools/verify
```

`make check` runs the same thing, and each mode below is also a make target (`make
sanitize`). `make help` lists the rest: `format`, `bundle`, `status` and `install`.

The default check runs `pack-model.py --check`, rustfmt, Clippy with warnings as errors, all
tests including doctests, rustdoc with warnings as errors, a release build and a C syntax check
of the header, and it links and runs the C ABI smoke test. On macOS it also checks native
formatting, lints the plists, runs Clang static analysis with findings as errors, builds the app
and tools, and runs the controlled native tests.

| Mode | Requires | What it does |
|---|---|---|
| `sanitize` | macOS | Native tests under ASan/UBSan; the Rust library is not instrumented |
| `browser-fixture` | Node 18+ | Deterministic JavaScript checks of the browser fixture |
| `audit` | cargo-deny 0.20.2 | Advisories, sources, licenses and bans for the app and fuzz graphs |
| `coverage` | cargo-llvm-cov 0.6.21, `llvm-tools-preview` | Rust line and region coverage, 95% floor |
| `evaluate` | Python 3 | Offline model evaluation, see [engine](engine.md#evaluation) |
| `fuzz` | cargo-fuzz 0.13.2, nightly-2026-10-26 | 10,000 text/parser/state and 200 model-asset cases, seed 126283 |
| `miri` | Miri and rust-src for nightly-2026-10-26 | FFI pointer and protocol tests |

Fuzzing prints a temporary directory with logs, evolved corpora and crash artifacts; replay a
crash with the same target and artifact path. Model fuzz inputs are bounded patches applied to
the real asset. Miri skips filesystem acquisition tests and the full immutable-model walk, which
takes about 26 minutes under Miri; those run in the normal suite. Coverage is Rust only; native
coverage doesn't count toward it.

The compatibility tests construct real Quartz events and need a WindowServer session. A
sandboxed shell can get null from `CGEventSourceCreate` even though the tests pass outside it;
report that as an environment failure rather than mocking the encoder.

## Style

Default rustfmt and Clippy lints, plus `missing_docs`, `clippy::missing_errors_doc` and
`clippy::missing_panics_doc`, with scoped `#[allow]` only where a reason is written next to it.
The core forbids `unsafe`; `keyrill-ffi` denies `unsafe_op_in_unsafe_fn` and every unsafe block
states its preconditions. Native code follows `.clang-format` (LLVM, 2 spaces); Rust uses
rustfmt's 4. `.editorconfig` gives editors those widths. Comment the why, not the what.

Public Rust items follow the [API Guidelines](https://rust-lang.github.io/api-guidelines/documentation.html):
a summary line, `# Errors` (and `# Panics` or `# Safety` where they apply), and a runnable
example on entry points. Examples are doctests, so `cargo test` compiles and runs them; use `?`
with hidden `# fn main() -> Result<...>` lines rather than `unwrap`. Error types implement
`std::error::Error` with content-free messages. Intra-doc links to macOS-only items break the
Windows docs build, so name those in plain code spans. See also the
[Rust Style Guide](https://doc.rust-lang.org/stable/style-guide/) and the
[Rustonomicon](https://doc.rust-lang.org/nomicon/ffi.html) for FFI.

Tests should constrain behavior: whole-document oracles for native schedules, exact reasons and
replacements for inference, property tests with fixed seeds, and no sleeps or wall-clock time in
state tests. See [testing](testing.md).

## Changes

Use [Conventional Commits](https://www.conventionalcommits.org/) with a scope where it helps
(`feat(macos):`, `fix(core):`), stage exact paths, and update docs in the same change as the
behavior. Native and ABI changes also need `./tools/verify sanitize` and a review of the actual
diff and its negative schedules. A policy or model change needs the evaluation rerun.

CI (`.github/workflows/verify.yml`) runs the default check on `macos-15`, `windows-2025` and
`ubuntu-24.04`, sanitizers on macOS, and separate audit, coverage, Miri, fuzz and evaluation jobs.
Actions are pinned by commit, the token is read-only and checkout doesn't persist credentials. No
job signs, publishes or installs anything. The coverage job attaches the single-page HTML report
(`target/llvm-cov/keyrill-coverage.html`), which opens in the browser from the run page, and puts
the per-file table in the run summary. Dependabot proposes Cargo and Actions updates monthly; the
Rust toolchains, the nightly date, the `cargo install` tool versions and the model generator's
Python requirements stay manual pins, the last because updating them means regenerating the model.
