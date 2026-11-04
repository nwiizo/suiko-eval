# suiko-eval

[suiko](https://github.com/nwiizo/suiko)の検出規則を、版を固定した評価集合で校正するCLIと評価データ。suikoのリポジトリを大きくしないよう分けている。

## 使い方

suikoの作業ツリーを隣に置いてビルドする（`Cargo.toml`の`suiko = { path = "../suiko" }`）。未リリースの規則もそのまま評価できる。

```sh
cargo +1.97 run --locked -- quality eval/corpus.toml --external   # 規則ごとの品質表と扱いの判定
cargo +1.97 run --locked -- report eval/corpus.toml --external --experimental
cargo +1.97 run --locked -- labeled eval/corpus.toml
cargo +1.97 run --locked -- fetch eval/sources.toml                # 著作権のある外部文書を取得し、lockと照合する
```

| コマンド | 内容 |
| --- | --- |
| `quality` | 規則ごとに人間文書での発火率とラベル付き標本の結果をまとめ、keep・降格候補・昇格候補・標本不足を返す |
| `report` | カテゴリ別の文書発火率とfinding件数（人間・AI、ジャンル別） |
| `labeled` | ラベル付き標本の検出率と誤検知率 |
| `sweep` / `calibrate` | 閾値候補の比較と、人間文書の誤検知上限を制約にした探索（devのみ） |
| `length-analysis` / `vocab` | 文書長別の件数、禁止語・誇張語彙の出現実測 |
| `feedback` | 推敲判断（採用・維持・保留・未確認・対応不能）の集計 |
| `fetch` | `sources.toml`のweb文書の取得と SHA-256 lock の記録 |

評価集合の構成と統計の前提は[eval/README.md](eval/README.md)、測定と判断の記録は[eval/calibration.md](eval/calibration.md)にある。外部文書の本文はコミットせず、`eval/corpus/external-lock.json`のSHA-256で照合する。

## 確認

```sh
cargo +1.97 fmt --check
cargo +1.97 clippy --locked --all-targets -- -D warnings
cargo +1.97 test --locked
```
