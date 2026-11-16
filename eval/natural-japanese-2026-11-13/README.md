# natural-japaneseとの比較（2026-11-13、suiko v0.4.2の公開前）

[coji/natural-japanese](https://github.com/coji/natural-japanese)（コミット`840bd40`、MITライセンス）の検出スクリプトとsuikoを、同じ文書で比べた記録。suikoは2026-09に同じリポジトリの`0f1cc1c`からコーパスの一覧と取得処理を取り込んでおり、lintの規則名の多くが共通している。本体は`upstream/`（gitignore）に取得し、このリポジトリにはコミットしない。

## 準備

```sh
git clone https://github.com/coji/natural-japanese.git eval/natural-japanese-2026-11-13/upstream
git -C eval/natural-japanese-2026-11-13/upstream checkout 840bd40
# natural-japaneseのlint（Python 3.10以上とsudachipyが要る。uvは一時実行で、設定は変えない）
mise exec uv@latest -- uv run eval/natural-japanese-2026-11-13/upstream/skills/natural-japanese/scripts/lint.py <file> --json
```

## 何が違うか

| 項目 | natural-japanese | suiko |
| --- | --- | --- |
| 形 | Agent Skill（SKILL.md）＋Pythonスクリプト（sudachipy） | Rust CLI＋Agent Skill |
| lintの規則 | 34区分 | 93区分（今回の`sentence_in_paren`を含む）。natural-japaneseの34区分のうち28区分は同名で持つ |
| natural-japaneseだけの規則 | `negation_repetition`、`sentence_in_paren`、`wago_nominalization`、埋め込みを使う`semantic_*`・`topic_jump_min` | — |
| 改稿の照合 | `newfacts.py`: 直した文書にだけある数値・固有名詞・英字の語の一覧 | `diff`（数値・否定・確信度・条件・頻度など30区分）、`check --material` |
| ほかの補助 | `calques.py`（場所の比喩の名詞と機械の処理に当てた動詞）、`outline.py`、`terms.py`、`paragraphs.py`、文書の種類ごとの手引き、`critical-thinking`（LLMによる論旨の検査） | `outline`、`terms`、`lexical-audit`、`academic`、`meaning`（LLMは任意） |

## lint: yomiyasuのコーパスでの発火

規則が多く重なっているので、両方の既定の規則を同じ文書に当てた（warn以上の文書数と件数）。

| | 人間 16 | 素のAI 26 | yomiyasuの推敲後 48 |
| --- | --- | --- | --- |
| natural-japanese | 4文書（8件） | 25文書（33件） | 45文書（54件） |
| suiko | 6文書（9件） | 25文書（33件） | 47文書（56件） |

素のAIでの内訳は両方とも`low_burstiness` 24件、`forbidden_phrase` 6件、`low_sentence_variance` 3件で同じだった。人間側の差は`low_burstiness`（natural-japanese 3件、suiko 6件）と`antithesis_repetition`（3件と1件）だった。natural-japaneseの検査用の例文（`fixtures/natural.md`）では、suikoも指摘0件だった。

## natural-japaneseだけの規則を測る

suikoに実験規則として入れ、suiko-evalの`quality`（人間95文書）と、生成文180文書（`slop-2026-11-06`）、yomiyasuの素のAI 26文書で文書単位の発火を数えた。

| 規則 | 人間 95 | 生成文 180 | 素のAI 26 | 扱い |
| --- | --- | --- | --- | --- |
| `negation_repetition`（「〜ではない」で終わる文が4文以上） | 9（上限0.170） | 0 | 0 | 入れない。人間の文書にだけ出る |
| `sentence_in_paren`（句点を含む括弧書き） | 16（上限0.256） | 0 | 0 | 読解負荷の指さしとして入れた。自然度の点数には入れない |
| `wago_nominalization`の「調べ」「測り」 | 0（`renyo_noun`全体） | — | 0 | `renyo_noun`の語に足した。「知らせ」は「お知らせ」として定着しているので入れない |

- `negation_repetition`は、natural-japaneseではwarnだが、どの集合でもAIの文書に出ず、人間の評論・随筆に出た。AI由来の手掛かりにならないので見送った。
- `sentence_in_paren`も人間側で多く、AI側で0件だった。natural-japaneseの説明どおり「括弧の中は読み飛ばされやすい」という読みやすさの指さしとして、`reading_load`へ置いた。

## diff: 元にない固有名詞

`newfacts.py`の考え方（直した文書にだけある名前を、元の文書全体と照らす）を、`diff`の`proper_noun_added`（warn）として入れた。書き換えた文に出た人名・地名のうち、変更前の本文のどこにもないものを示す。

- 最初はSudachiの「固有名詞」をすべて数えた。yomiyasuの推敲前後48組で3件出て、そのうち「DB」と「フロントエンド」は「固有名詞・一般」の一般の語だった。人名・地名だけに絞り、変更前の本文に同じ文字列があれば数えないようにした。
- 絞ったあとは48組で1件（「（JST）」→「日本時間の」）。言い換えとして正しいが、元にない地名を足した形なので確認する対象に残した。
- 数値と英字の識別子は、既存の`number_changed`・`identifier_changed`が受け持つ。

## 入れなかったもの

- 埋め込みを使う`semantic_*`・`topic_jump_min`: 決定的なCLIの外にあるモデルが要る。suikoのLLMの確認は`meaning`で任意にしている。
- `calques.py`: 「拾う」「通る」などの動詞を広く候補にし、本人も「誤検出は多めに出る前提」としている。suikoは`explanatory_metaphor`・`technical_jargon_metaphor`で対象と述語の組を絞って持っている。
- `critical-thinking`: LLMで論旨を検査するスキルで、決定的な検出の対象ではない。
