# ci-preflight Design Document

## 1. Overview

`ci-preflight` is a **lightweight preflight checker for GitHub Actions**.

It performs a *best-effort static interpretation* of GitHub Actions workflows to estimate whether **test jobs can run successfully**, **without executing CI**.

The tool prioritizes:

* fast feedback
* explicit uncertainty
* semantic clarity over environment reproduction

---

## 2. Non-goals

This project explicitly does **not** aim to:

* Reproduce GitHub Actions environments
* Execute workflows faithfully
* Replace CI or test runners
* Fully interpret shell scripts
* Support all GitHub Actions features
* Support Windows runners (initially)

These are intentional design exclusions.

---

## 3. Core Design Principles

### 3.1 Preflight, Not Execution

`ci-preflight` never attempts to replace CI execution.

It answers:

> *“Is it likely that this test job can run?”*

not:

> *“Does this workflow succeed?”*

---

### 3.2 Unknown Is a First-Class Outcome

Uncertainty is not treated as an error.

Unknown information is:

* explicitly surfaced
* classified structurally
* propagated conservatively

Failing fast is preferred only when **test execution is impossible**.

---

### 3.3 AST-First Architecture

Both GitHub Actions workflows and `run:` scripts are treated as **domain-specific languages**.

Parsing produces **ASTs designed for semantic interpretation**, not execution.

---

### 3.4 Step-Level Semantics

All semantic evaluation is defined at the **step level**.

Higher-level nodes (Job, Workflow) only **aggregate** results.

---

### 3.5 Conservative Evaluation

Control flow is never executed or conditionally evaluated.

All branches are analyzed and **worst-case results are merged**.

---

## 4. Architecture Overview

```
YAML (workflow)
   ↓
Actions AST
   ↓
Step Evaluation
   ↓
Job Reduction
   ↓
Workflow Reduction
   ↓
Preflight Result
```

---

## 5. Actions AST

### 5.1 Minimal AST Structure

```text
Workflow
 └─ Job*
     └─ Step*
         ├─ Run
         └─ Uses
```

This structure is intentionally minimal.

---

### 5.2 Step Variants

```text
Step =
  | Run
  | Uses
```

* `Run`: shell-based steps (`run:`)
* `Uses`: action-based steps (`uses:`)

Composite Actions や `uses: docker://`、matrix 展開、再利用ワークフローは現状未対応とし、必要に応じて UNKNOWN 扱いに落とす。

---

## 6. Run AST (Shell Subset)

### 6.1 Purpose

The Run AST represents **recognized shell structure**, not executable logic.

Its purpose is to:

* extract intent
* infer required tools
* detect unsafe or unknown constructs
* bound uncertainty

---

### 6.2 Supported Structural Nodes

```text
Stmt =
  | SimpleCmd
  | Seq
  | And
  | Or
  | If
  | For
  | FunctionDef
  | FunctionCall
  | Subshell
  | Unknown
```

Key properties:

* Parsing attempts to **recognize structure whenever possible**
* Unsupported constructs become **structured Unknown nodes**
* Parsing failure must not abort analysis

---

### 6.3 Unknown Classification

Unknown nodes are **classified**, not opaque.

Examples:

* External script invocation
* Function definition
* Subshell execution
* Here-doc usage

This enables future extensions without re-parsing.

分類スキーマは列挙型で保持し、拡張時は新しい Unknown 種別を追加する方針。

---

## 7. Evaluation Model

### 7.1 Evaluation Result Type

Each step produces an evaluation:

```text
Evaluation {
  intent     : setup | test | other | unknown
  tools      : ToolRequirement*
  safety     : SAFE | CAUTION | DANGEROUS
  result     : PASS | FAIL | INCONCLUSIVE | SKIPPED
  confidence : 0.0 .. 1.0
  notes      : Reason*
}
```

Confidence 算出は当面ヒューリスティックで運用し、後日チューニング予定。

---

### 7.2 Intent Classification

| Intent  | Meaning                    |
| ------- | -------------------------- |
| setup   | Environment preparation    |
| test    | Determines success/failure |
| other   | Known but irrelevant       |
| unknown | Cannot classify intent     |

Intent and uncertainty are **independent dimensions**.

複数の手掛かりが矛盾する場合はスコアベースで最大スコアを採用し、決めきれない場合は UNKNOWN にフォールバックする。

---

### 7.3 Tool Inference

Tool requirements are inferred from:

* command names
* setup actions
* explicit references

Tool presence is validated **before execution** via `which` などの静的検出を想定。欠落が判明した必須ツールは該当ステップを即 FAIL とする。

---

### 7.4 Safety Classes

| Class     | Description                   |
| --------- | ----------------------------- |
| SAFE      | No side effects (e.g. `echo`) |
| CAUTION   | May affect environment        |
| DANGEROUS | Modifies system state         |

Safety affects execution eligibility, not intent.

境界例（例: ファイル生成・キャッシュ書き込み・`docker build` 等）は副作用の有無と重さで判定し、疑わしい場合は CAUTION もしくは DANGEROUS に寄せる。

---

## 8. Control Flow Semantics

### 8.1 Non-execution Rule

Control flow constructs are **never executed**.

---

### 8.2 Conservative Merge

For constructs like `if` or `for`:

* All branches are evaluated
* The **worst result** is propagated
* Confidence is reduced

Early-exit 意図（例: `set -e`, `|| exit 1`）は将来的に特別扱いするTODO。

---

## 9. Reduction Rules

### 9.1 Job Reduction

```text
FAIL > INCONCLUSIVE > PASS > SKIPPED
```

* Any FAIL → FAIL
* Any INCONCLUSIVE → INCONCLUSIVE
* Otherwise PASS or SKIPPED

Confidence is the **minimum** of step confidences.

---

### 9.2 Workflow Reduction

Workflow result is the worst job result.

---

## 10. OS and Shell Assumptions

* Target runner OS is derived from `runs-on`
* Local execution OS may differ
* OS mismatch reduces confidence
* Shell defaults are inferred conservatively

OS mismatch 時の減衰量は後日設計する。

Windows runners are currently **out of scope**.

---

## 11. CLI Philosophy

`ci-preflight` is:

* fast
* read-only
* explainable

### Core flags:

* `--strict`: fail on unknowns
* `--explain`: show reasoning
* default mode: best-effort

---

## 12. Rationale for Rust

Rust is chosen because:

* ASTs are closed worlds
* Exhaustive matching enforces semantic completeness
* Unknown kinds evolve safely
* Design invariants are enforced by the type system

---

## 13. Future Extensions

Potential future work includes:

* Partial script following
* Composite action inspection
* `uses: docker://` support
* Matrix / reusable workflow expansion
* Confidence calibration
* Windows support
* CI design diagnostics

None are required for v0.1.

---

## 14. Summary

`ci-preflight` is a **semantic interpreter**, not an executor.

It provides:

* fast feedback
* explicit uncertainty
* conservative correctness

Its design favors **clarity over completeness** and **soundness over fidelity**.
