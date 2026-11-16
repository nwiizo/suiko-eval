# yomiyasuのテスト用コーパスによる評価（2026-11-06、suiko v0.4.2の公開前）

suikoの規則の調整に使っていない外部の文書で、v0.4.2の`lint`と`diff`を測った記録。対象は[nanaism/yomiyasu](https://github.com/nanaism/yomiyasu)の`tests/corpus`（コミット`8394e09`、MITライセンス）。本文はこのリポジトリにコミットせず、`upstream/`（gitignore）に取得して、manifestのSHA-256で照合する。

## 準備

```sh
git clone https://github.com/nanaism/yomiyasu.git eval/yomiyasu-2026-11-06/upstream
git -C eval/yomiyasu-2026-11-06/upstream checkout 8394e09
cargo +1.97 run --locked --release -- report eval/yomiyasu-2026-11-06/raw.toml --experimental
cargo +1.97 run --locked --release -- report eval/yomiyasu-2026-11-06/rewritten.toml --experimental
```

| manifest | human | ai |
| --- | --- | --- |
| `raw.toml` | `human/` 16文書（技術メモ8・実務エッセイの抜粋8、各200〜650字） | `raw_ai/` 26文書（Sonnetの生成文。技術・障害報告・PR・告知・比較・仕様・レビュー・振り返り） |
| `rewritten.toml` | 同じ16文書 | `yomiyasu_rewritten/` のうち `raw_ai` を推敲した24文書（`_bl`を除く） |

ジャンルは、技術メモと`01`〜`03`・`05`〜`07`・`09`をtech、エッセイと`08`・`10`をessay、`04`（Slackの告知）をbusinessとした。labelは由来の区別で、品質の評価ではない。

## lint: 文書単位の発火

当たった規則だけを示す（`--experimental`、文書数（件数））。

| 規則 | 人間 16 | 素のAI 26 | 推敲後 24 |
| --- | --- | --- | --- |
| `low_burstiness` | 6 (6) | 24 (24) | 23 (23) |
| `forbidden_phrase` | 2 (2) | 4 (9) | 2 (2) |
| `repeated_syntax_template` | 1 (3) | 2 (11) | 0 |
| `short_topic_comma` | 1 (1) | 1 (1) | 5 (5) |
| `low_sentence_variance` | 0 | 3 (3) | 5 (5) |
| `paragraph_register_mixture` | 0 | 3 (3) | 2 (2) |
| `sentence_too_long`（読解負荷） | 3 (3) | 1 (1) | 0 |
| 書式の規則（`bullet_bold_label`・`heading_colon`・`heading_emoji`・`high_bold_density`・`boilerplate_heading`） | 0 | 各1 (1) | 0 |
| `evaluation_fragment_run`・`negative_listing`・`paragraph_lead_conjunction`・`high_length_autocorrelation` | 0 | 各1 (1) | 0 |
| `antithesis_repetition`・`redundant_light_verb`・`translationese_morph`・`uniform_paragraph_structure` | 各1 (1) | 0 | `uniform_paragraph_structure`のみ1 (1) |

- 素のAI文書は短く、比喩や擬人化の規則は1件も当たらなかった。素のAI文書と推敲後で差があったのは、書式の規則と`repeated_syntax_template`（11件→0件）である。
- `low_burstiness`は、短い人間の文書でも6/16に当たった。この規則の条件は6文以上だけで、品質表で降格候補になっている（人間95文書中28件）こととも合う。
- 推敲後に`short_topic_comma`が1件から5件に増えた。yomiyasuの推敲が短い主題のあとに読点を打った文（「停止中は、」「運用担当は、」「開発者は、」）で、意味の誤りではない。
- 文書数が少なく、Wilsonの区間が広い。率を規則の精度として引用しない。

## lint: 自然な用法の境界例

`edge_cases/`の55文書（「握る」「倒す」「溶かす」「割る」「壊れる」「効く」などの字義どおりの用法）に、`--genre tech --experimental`で`lint`を当てた。文書全体の統計を除くと、指摘は1文書の2件（`translationese_morph`「ことができ」、`redundant_light_verb`「練習を行い」）だけで、比喩の規則は0件だった。

## diff: 推敲の前後48組

`raw_ai/X.md`→`yomiyasu_rewritten/X.md`の24組と、`blacklist_ai/X.md`→`yomiyasu_rewritten/X_bl.md`の24組に`suiko diff --json`を当てた。全組が終了コード0で終わった。

| 区分 | 件数 | 読んだ結果 |
| --- | --- | --- |
| `subject_added`（warn） | 42 | yomiyasuの推敲が「システムは」「ワーカーは」「運用担当は」など、原文にない主語を足した箇所。原文にない動作主の追加として読み合わせる価値がある |
| `negation_changed`（warn） | 28 | 28件とも意味を保った言い換え（「防ぐ」→「しないようにする」、「止まった」→「進まなかった」、「正常化」→「出なくなった」）。否定の語の数だけを比べる方法の限界 |
| `sentence_function_changed`（warn） | 22 | 依頼（「ご確認ください」）が説明に変わった文など、実際に働きが変わった例を含む。「流せません」→「実行できません」のように同じ働きの言い換えも含む |
| `identifier_changed`（warn） | 17 | ほとんどが、推敲で足した「API」「DLQ」などの語。主語の追加と重なる |
| `number_changed`（warn） | 13 | 「2秒を超え」→「2秒以上」のような範囲の変化を含む。「30日」→「30日間」のように、期間の書き分けだけの3件は値が同じ |
| `certainty_changed`（warn） | 11 | 「再送する」→「再送することがある」のように確度が変わった例を含む |
| `sentence_removed`（warn） | 10 | 「■ 日時」のような見出しの記号の行を消した箇所と、内容を別の文へまとめた文（「再ログインを促す状態へ遷移させます」→後の段落の「再ログインが必要な状態に切り替えます」） |
| `addressee_changed`（warn） | 1 | 見出し「Webhook受信のリトライ制御とDLQ仕様」の「様」を宛名とした誤検知 |
| infoの候補 | 248 | `content_words_changed` 126、`causal_changed` 42、`structure_changed` 30、`condition_changed` 30 など |

対応:

- `addressee_changed`の誤検知は、suikoで直した（「仕様・同様・模様」等で終わる行を宛名にしない）。直したあとの再実行では0件だった。
- `negation_changed`の言い換えは、2026-11-07に直した（下の「suikoへの取り込み」。28件→6件）。期間の「日」と「日間」の扱いは、仕様を決める必要があるため、suikoのTODOに残した。資料照合のテストは、「3日間」と「3日」を別の値として固定している。

## 既存の評価集合での回帰確認

同じ日に、`quality`・`labeled`・`report`・`spans`を、suikoの`4ce6fee`（修正前）とv0.4.2の作業ツリーで実行し、出力が一致することを確かめた。外部のweb文書は81件中37件の本文が2026-08-18の取得時から変わっていたため、`fetch`で取り直して`external-lock.json`を更新し、前後とも同じ本文で比べた。品質表は keep 15・降格候補 9・実験維持 6・標本不足 54で、[2026-11-06の記録](../calibration.md#fix_rule-の4規則の修正2026-11-06)と同じだった。

## yomiyasu 2.5.0の検査との比較（2026-11-07）

yomiyasu（`8394e09`、2.5.0）の`yomiyasu_lint.py`と`yomiyasu_diff.py`を、suiko v0.4.2の作業ツリーと同じ文書に当てた。どちらも既定の設定で、yomiyasuは`--doc-type`、suikoは`--genre`と`--experimental`を付けていない。yomiyasuはPython標準ライブラリの正規表現と語の表で判定し、形態素解析を使わない。yomiyasuのテスト（2,431件）は、macOSのPython 3.9.6で1件だけ失敗した（`test_v213_tools`の待ち時間の上限1.5秒に対して約3秒。単独で実行しても再現した）。

### 人間の文書95件のlint

`eval/corpus`の人間の文書（外部の81件と、青空文庫など14件）。

| | yomiyasu | suiko |
| --- | --- | --- |
| warn以上の指摘がある文書 | 67 | 55 |
| 多い規則（文書数） | `sentence_end_repetition` 33、`slop_vocabulary` 23、`emoji_prohibited` 23、`dash_decoration` 16、`trailing_colon` 8、`meta_filler` 8 | `forbidden_phrase` 34、`low_burstiness` 28、`antithesis_repetition`（critical 6、warn 4）、`duplicate_comma` 3 |

- yomiyasuの`slop_vocabulary`は語を文字列の一部として探すため、「ゲートル」に「ゲート」、随筆の「本質的」「真理」「体温」にも当たった。`emoji_prohibited`の多くは、取得したweb文書の著者名の絵文字だった。`dash_decoration`は古典の「――」と、範囲の「十一―十二月」に当たった。
- 2つの道具の多い規則は重ならない。yomiyasuは書式と文末の形、suikoは語と文書全体の統計で人間の文書に当たる。

### yomiyasuのコーパスでのlint

warn以上の指摘がある文書の数（件数）。suikoの指摘の多くは`low_burstiness`である（人間6、素のAI 24、推敲後47）。

| 群 | yomiyasu | suiko |
| --- | --- | --- |
| `human` 16 | 3（3） | 6（9） |
| `raw_ai` 26 | 8（18） | 25（33） |
| `blacklist_ai` 24 | 6（12） | 23（27） |
| `yomiyasu_rewritten` 48 | 9（19） | 47（56） |
| `edge_cases` 55 | 0 | 0 |

yomiyasuの推敲後の文書48件のうち9件に、yomiyasu自身の`sentence_end_repetition`が19件付いた（推敲の出力は古い版のyomiyasuで作ったもの）。

### 生成文180文書の注釈50件

`eval/slop-2026-11-06`の注釈の範囲を含む行に、読点の2規則を除く指摘があるかを行の単位で数えた。随筆の段落は1行が長いので、行の単位は範囲の重なりより多く数える。suikoの範囲の重なりでの値は[slop-2026-11-06](../slop-2026-11-06/README.md)を見る。

| | 行が重なった注釈 | 指摘の数 |
| --- | --- | --- |
| yomiyasu | 4/50 | 130 |
| suiko | 19/50 | 429 |

yomiyasuの比喩動詞（`metaphor_verb`）は180文書で2件だけだった。一覧の語（壊れる、倒す、効く、溶かすなど）は技術文書の比喩を対象にしており、随筆や小説の比喩の破綻は拾わない。

### 推敲の前後48組のdiff

| yomiyasu | 件数 |
| --- | --- |
| 意味が変わる恐れ | 20（`number` 6、`certainty_review` 6、`sentence_type` 5、`promise` 1、`negation` 1、`modality_review` 1） |
| `--fail-on meaning-risk`に数える候補 | 13（10組） |
| 原文にない人や担当を足した文 | 34 |

suikoのwarnは`subject_added` 42、`negation_changed` 28（取り込み後は6）、`sentence_function_changed` 22、`identifier_changed` 17、`number_changed` 13などである（上の表）。yomiyasuの否定の規則と「原文にない人や担当」の語の一覧は、この48組を見て決めたとyomiyasuの`docs/limitations.md`にあり、yomiyasuの値は取っておきの結果ではない。

### 合成の組

1文ずつの組で、warn以上（yomiyasuは意味が変わる恐れ）の候補を比べた。

| 前 → 後 | yomiyasu | suiko |
| --- | --- | --- |
| 二重処理を防ぐ → 二重に処理しないようにする | なし | `negation_changed` |
| 二重課金を防ぐ → 二重課金を防がない | `negation` | `negation_changed` |
| 保存します → 保存しません | `negation` | `negation_changed` |
| 田中が佐藤を承認した → 佐藤が田中を承認した | `swap` | `subject_object_swapped` |
| 30日保持する → 30日間保持する | なし | `number_changed` |
| 管理者だけが使える → 管理者だけが使っている | なし | `sentence_function_changed` |
| 原因は調査中です → 原因を特定しました | `state/lost` | `certainty_changed` |
| AまたはBのどちらかが必要 → AとBの両方が必要 | `choice/one_to_all` | なし |
| 正常化を確認した → タイムアウトが出なくなったことを確認した | なし | `negation_changed` |
| 約100件 → 100件 | `number_qualifier/lost` | `number_changed` |

- 否定は、yomiyasuが文字の差分で変わった箇所だけを数えるため、言い換えに出さず、本当の反転は拾う。
- 選ぶ範囲の入れ替えは、この時点のsuikoに対応する区分がなかった。

### suikoへの取り込み（2026-11-07）

上の比較を受けて、v0.4.2の作業ツリーで次を直した。

- `negation_changed`: 文末の述語の肯定・否定（「防ぐ」「避ける」「防止」「なくなる」「止まる」などを否定と同じ向きに数える）が入れ替わった文と、字面がほぼ同じで両方にある語の打ち消しが増減した文に絞った。推敲の前後48組で28件→6件。合成の組の「防ぐ→防がない」「保存します→保存しません」「確認せずに保存する→確認して保存する」は引き続き示す。
- `choice_changed`（warn）を加えた。合成の組の「どちらか→両方」「すべて→いずれか」を示し、48組では0件だった。
- 「関連」「相関」「関係」だけを述べた文を因果にした`causal_changed`をwarnにした。48組では0件だった。
- `lint`の`revision_expression`の「が招く」に、失敗・事故・損失などの名詞を足した。

既存の評価集合の`quality`・`labeled`・`report`・`spans`と、`raw.toml`の`report`は、取り込みの前後で出力が一致した。48組の残る否定の6件は語ごとの言い換えで、suikoのTODO（述語ごとの照合）に残した。

yomiyasuにあってsuikoに取り込まなかったもの:

- 返答の終わりと投稿の前に検査記録を照らすフック（`yomiyasu_hook.py`）と、検査したものと同じバイト列の写しを作る`export`。suikoの`check`・`verify`は記録の照合までで、ホストのフックには入らない。
- 文書の種類13種（`--doc-type`）で規則を外す仕組み。suikoはジャンル3種と`.suiko.toml`の`disabled_rules`で扱う。
- 文末の繰り返し・文末のコロン・本文の絵文字・ダッシュ・語の一覧の部分一致（`sentence_end_repetition`・`trailing_colon`・`emoji_prohibited`・`dash_decoration`・`slop_vocabulary`）。人間の文書95件で8〜33文書に当たり、誤検知が多い。
- 「美しい、手触り。」のような短い句の体言止め（`copy_comma_noun`）、直訳語の型2・型4（入口、版）。誤検知を確かめる標本がまだない。
- yomiyasuの`_case_map`も「仕様」「同様」の「様」を敬称として外さないようにしている。v0.4.2で直した`addressee_changed`の誤検知と同じ形である。

## 1文の組でyomiyasuのdiffと比べる（2026-11-09）

yomiyasuの`docs/tools.md`が「意味が変わる恐れ」として挙げる書き換えと、どちらの道具も対象にしていなかった頻度・曜日・例示・順序を、1文ずつの組40組にした（[synthetic-pairs.tsv](synthetic-pairs.tsv)、この記録のために書いた文）。各組をファイル2つに書き、yomiyasu（`8394e09`）の`yomiyasu_diff.py --json`の`meaning_risks`と、suikoの`diff --json`のwarn（`content_words_changed`を除く）を比べた。

suikoの取り込み前（2026-11-07の作業ツリー）の結果:

| 区分 | 組 |
| --- | --- |
| 両方が示した | 22 |
| yomiyasuだけ | 3（未実装です→実装します、「保証する」を足した文、p<0.05→p>0.05） |
| suikoだけ | 7（以上→超、最大5回まで→5回、だけ・のみが消えた文2組、すべて→一部、義務→勧め、30日→30日間） |
| suikoの誤検知 | 1（と思われます→見込みです。yomiyasuは同じ強さの推量として示さない） |
| どちらも示さない | 7（毎日→毎週、ときどき→いつも、金曜→土曜、PDFなど→PDF、AしてからB→BしてからA、した→する、かもしれません→恐れがあります。最後の組は示さないのが正しい） |

これを受けて、suikoの`diff`を次のように直した。

- 名詞の述語の打ち消し（未実装です→実装します、不要だ→必要だ）を`negation_changed`に数える。
- 比較の記号（<、>、≤、≥）と曜日（「金曜」「金曜日」「3日(金)」は同じ値）を`number_changed`の値として比べる。
- 「保証する」「担保される」を`promise_added`に加え、保証・担保・再発防止・「二度と」・「約束します」はwarnにする。
- 推量（かもしれない、と思われる、見込み、恐れがある、おそらくなど）を1つの種類として1文に1つと数え、推量どうしの言い換えは`certainty_changed`に出さない。推量を外して言い切った文は新しく示す。
- `frequency_changed`（頻度の語が替わるか消えればwarn）を加えた。「毎回」→「〜するたびに」は示さない。
- `condition_changed`に例示（など、といった、をはじめ）を加え、例示の語がすべて消えた場合だけwarnにした。
- 「30日」→「30日間」のように期間の「間」の有無だけが違う場合は`number_changed`をinfoにした（「3日」は日付とも読めるため、同じ値とはみなさない）。

直したあとは、40組のうち35組でsuikoがwarnを返した。返さない5組は、推量どうしの言い換え2組と、30日→30日間（info）、した→する（info）、順序の入れ替え（どちらも示さない）である。

推敲の前後48組では、warnの合計は136件で変わらなかった。

| 区分 | 取り込み前 | 取り込み後 | 中身 |
| --- | --- | --- | --- |
| `number_changed` warn | 13 | 11 | 30日→30日間、14日→14日間の2件がinfoへ |
| `condition_changed` warn | 6 | 7 | 「…フィールド単位の認可といった追加の統制」の例示が消えた文 |
| `frequency_changed` warn | — | 1 | 「本チャンネルで随時お知らせします」→「お知らせします」 |
| `condition_changed` info | 30 | 33 | 例示の語の言い換えと増加 |
| `frequency_changed` info | — | 1 | 「頻繁に」を足した文 |

途中で見つけた誤検知（「冪等」の「等」を例示に、「10月3日(土)」→「10月3日土曜日」を曜日の変化に、「〜するたびに」を足した文を頻度の追加に数えていた）は、直してから数えた。`quality`・`report`・`labeled`・`spans`と`raw.toml`の`report`は、変更の前後で出力が一致した。
