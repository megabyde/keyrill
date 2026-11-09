# ADR 001: Rust core, native adapters

> [!TIP]
> **Status:** accepted.

Conversion, segmentation, vocabulary, scoring, policy and the word/reversal state live in one Rust
2024 crate that forbids `unsafe` and has no OS dependency. Each platform gets its own native
adapter: Objective-C++ with ARC for macOS InputMethodKit, and whatever the platform's input
framework needs on Windows and Linux. They meet at a narrow, versioned C ABI with opaque handles,
fixed-width tags, explicit lengths and copied buffers; raw pointers and panic containment stay in
`keyrill-ffi`.

The alternatives were a C++ core, logic duplicated per platform, and a pure-Rust IMK adapter through
`objc2`, which adds interop and lifetime work without a benefit over native Objective-C++.

No document-editing abstraction is shared across platforms. IMK, TSF and Linux frameworks differ in
locks, callbacks, focus, source selection and composition identity, and a common facade would hide
those differences instead of proving each one. Native code is still unsafe in the broader sense even
though the core is not.

Revisit the boundary only with concrete evidence it doesn't hold up, and a shared editing
abstraction only after two working adapters expose the same contract.
