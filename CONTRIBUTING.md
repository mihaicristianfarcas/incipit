# Contributing

Thank you for helping. Incipit is young, so the best contributions come
with a reason and a way to check them.

## Reporting a problem

[Open an issue](https://github.com/mihaicristianfarcas/incipit/issues/new/choose)
with the bug form: what you did, what happened, what you expected, and your
system. Security problems go privately instead: see
[SECURITY.md](SECURITY.md).

## Proposing a change

For anything larger than a fix, open an issue first and say what you want
to change and why: a design that fits on the first try saves both of us
time.

## Making the change

1. Set up a build: `tools/setup`, then
   [docs/development.md](docs/development.md).
2. Read [AGENTS.md](AGENTS.md): the project's rules for code, comments,
   documentation and commits. They hold for people and AI agents alike.
3. Work on a branch. `main` takes changes only through pull requests.
4. Before you open the pull request:

   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

5. Open the pull request with the template: what changed, why, how you
   checked it, and what you could not check.

A pull request merges by rebase once CI passes and its conversations are
resolved.

## Using an AI agent

Agents are welcome. You are responsible for what yours writes: read it,
run it, and say in the pull request what was verified and how.

## License

By contributing you agree that your contribution is licensed under the
[AGPL-3.0](LICENSE).
