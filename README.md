# ci-preflight

`ci-preflight` は、GitHub Actions Workflow を実行せずに静的解析し、
「必要ツールの事前確認」と「コマンド意図の可視化」を行う軽量 CLI です。

## 何ができるか

- Workflow YAML を Actions AST + Shell AST として解析
- `run:` / `uses:` から必要ツールを推定し、ローカル PATH 上の有無を検査
- `run` 内コマンドと `uses` ステップに `CmdKind`（`EnvSetup` / `TestSetup` / `Test` / `Assert` / `Other`）を付与して表示
- 未知の `uses`・不明コマンドを `unknown_*` として明示

設計上、CI の完全再現ではなく「高速な事前検証（preflight）」を目的にしています。

## 現在の CLI

```bash
ci-preflight --parse-only <FILE>
ci-preflight --check-tools <FILE>
ci-preflight --print-cmd-kind <FILE>
```

- `--parse-only`: Workflow 構造をデバッグ表示
- `--check-tools`: 必要ツールを列挙し、PATH 上にない場合は終了コード `2` で失敗
- `--print-cmd-kind`: 元 YAML に ` --- CmdKind` 注釈を付けて表示（色付き）

## クイックスタート

### 1. ビルド

```bash
cargo build
```

### 2. 解析のみ

```bash
cargo run -- --parse-only test/unit_test.yml
```

### 3. 必要ツール検査

```bash
cargo run -- --check-tools test/unit_test.yml
```

出力例:

```text
required: cargo, git
found: cargo, git
missing:
unknown_commands:
unknown_uses:
PASS: all required tools are installed
```

### 4. CmdKind 注釈表示

```bash
cargo run -- --print-cmd-kind test/uses_mixed.yml
```

出力イメージ（抜粋）:

```yaml
- uses: actions/checkout@v4 --- EnvSetup (Checkout)
- uses: actions/setup-node@v4 --- EnvSetup
- uses: octo-org/custom-action@v1 --- Other
- run: |
    echo ok --- Other
    cargo test --- Test
```

## サポート範囲と方針

- Workflow / shell は一部サブセットを対象にした保守的解析です
- 未対応・不確実な構文は失敗させるよりも `unknown` として残します
- `unknown_uses` は現時点では FAIL 条件ではありません（`missing_tools` のみ FAIL 判定）
- `uses` の既知情報は `data/action_catalog.yaml` で管理します（`owner/repo` 単位）

## 非ゴール（現時点）

- GitHub Actions 環境の完全再現
- Docker / VM ベースの忠実実行
- すべての Workflow 機能（composite / `docker://` / matrix / reusable workflow など）の網羅
- Windows ランナー対応
