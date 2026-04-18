# Contributing

Thanks for your interest in Stellar MLS. Contributions are welcome.

## How to contribute

1. **Open an issue** at https://github.com/rinat-enikeev/stellar-mls/issues to report a bug, request a feature, or discuss a change before you start work on anything non-trivial.
2. **Open a pull request** against `main` with your change. Link the issue it closes or relates to.

That's it. No CLA, no style committee, no required templates.

## Using AI

AI-assisted contributions are welcome. Claude Code, Copilot, Cursor, and similar tools are all fine. A few expectations:

- **You are responsible for every line you submit.** Read the diff, understand it, and make sure it does what you think it does. "The model wrote it" is not a defense for a broken PR.
- **Test it.** Run the affected tests locally (`cargo test` for Rust, the Swift/Kotlin test targets for the SDKs). Do not rely on CI to find basic breakage.
- **Don't submit AI slop.** Generated docs, README fluff, speculative refactors, or changes that "improve" working code without a concrete reason will be closed.
- **Security-sensitive code needs extra care.** Circuits, verifier logic, key handling, and the relayer are not places to accept model output uncritically. If an AI wrote it, review it twice.

## What to keep in mind

- **Scope:** this is a privacy-critical cryptographic system. Small, focused PRs land faster than large ones. If your change touches the circuits, verifier, or epoch logic, expect a slower review.
- **Documentation:** if you change behavior documented in [`docs/sep.md`](docs/sep.md), [`docs/design-doc.md`](docs/design-doc.md), or any of the post-mortems, update the doc in the same PR.
- **Security issues:** do not file them as public issues or PRs. See [`SECURITY.md`](SECURITY.md).

## License

By contributing, you agree that your contribution is licensed under the MIT license (see [`LICENSE`](LICENSE)).
