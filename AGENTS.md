# Repository Guidelines

## Project Structure & Module Organization
- Core Rust code lives in `src/`.
- Workflow parsing logic is in `src/actions_parser/`:
  `parser.rs` (GitHub Actions YAML),
  `sh_parser/` (shell lexer/parser/AST),
  `actions_ast.rs` (workflow AST types),
  `arena.rs` (`AstArena` + `AstId` allocator),
  `source_map.rs` (`SourceMap` and `SourceId`).
- Public entrypoints are `parse_actions_yaml` and `format_actions_tree` in `src/actions_parser.rs`.
- CLI entry point is `src/main.rs`; analysis scaffold is in `src/analysis.rs`.
- Test fixtures currently live in `test/` (for example `test/unit_test.yml`).
- Build artifacts are generated in `target/`.

## Build, Test, and Development Commands
- `cargo build` : compile the project.
- `cargo run -- --parse-only test/unit_test.yml` : parse a sample workflow and print the AST tree.
- `cargo test` : run tests.
- `cargo fmt` : format source code with rustfmt.
- `cargo clippy --all-targets --all-features` : run lints before opening a PR.
- Recommended pre-push check: `cargo fmt && cargo clippy --all-targets --all-features && cargo test`.

## Coding Style & Naming Conventions
- Follow Rust defaults: 4-space indentation, `snake_case` for functions/modules/files, `CamelCase` for types/enums.
- Keep parser changes localized: AST shape changes in `actions_ast.rs`, YAML decoding in `parser.rs`, shell grammar in `sh_parser/`.
- Preserve the arena model: always allocate nodes via `AstArena::alloc_actions` / `AstArena::alloc_sh`, and pass references by `AstId`.
- On unsupported syntax, prefer structured fallback (`ShAstNode::Unknown`) plus collected parse errors over hard failure.
- Use `Result`-based propagation and typed errors (`thiserror`); keep CLI diagnostics through `color-eyre`.
- Do not commit debug-only prints unless they are intentional CLI output.

## Testing Guidelines
- Use `cargo test` as the primary test entrypoint (also used in CI).
- Place parser unit tests near parser code (`src/actions_parser/parser.rs`).
- Add integration tests under `tests/` when behavior spans CLI + parser.
- Name tests by behavior, e.g. `parses_run_step_with_if_branch`.
- For parser updates, include at least one valid case, one invalid case, and one regression around fields like `runs-on`, `steps`, `if`, `container`, or `services`.

## Commit & Pull Request Guidelines
- Current history is short (`impl parser`, `wip`); use imperative commit messages.
- Recommended format: `<area>: <what changed>` (example: `parser: handle run step without name`).
- Keep commits scoped and buildable.
- PRs should include: purpose, key changes, test evidence, and linked issue/task.

## Security & Configuration Tips
- Treat workflow files as untrusted input: keep parser behavior defensive and non-executing.
- Avoid introducing commands that execute workflow scripts during analysis.
- Use `mise.toml` (`rust = "latest"`) to align local toolchain.
