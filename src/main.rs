use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use suiko_eval::{self as evaluation, SplitFilter, SweepRule};

#[derive(Debug, Parser)]
#[command(
    name = "suiko-eval",
    version,
    about = "Suikoの検出器を再現可能な評価集合で校正する"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 明示された推敲の採否を規則別に集計する（未確認を不採用にしない）
    Feedback { decisions: PathBuf },
    /// sources.tomlのweb文書を取得し、本文とSHA-256 lockを保存する
    Fetch {
        sources: PathBuf,
        /// このidだけを取得する
        #[arg(long)]
        id: Option<String>,
        /// 先頭N件だけを取得する
        #[arg(long)]
        limit: Option<usize>,
    },
    /// カテゴリ別の文書発火率とfinding件数を表示する
    Report {
        manifest: PathBuf,
        #[arg(long)]
        experimental: bool,
        /// 対象split。holdoutは閾値確定後の一度きり評価にだけ使う
        #[arg(long, value_enum)]
        split: Option<SplitFilter>,
        /// sources.tomlの外部取得文書(ローカル、lock一致分)を含める
        #[arg(long)]
        external: bool,
    },
    /// 選択した検出器の閾値候補を比較する
    Sweep {
        manifest: PathBuf,
        #[arg(long, value_enum)]
        rule: SweepRule,
        #[arg(long, value_delimiter = ',', required = true)]
        values: Vec<f64>,
        #[arg(long)]
        experimental: bool,
    },
    /// 文書長別に文書数とfinding件数を表示する
    LengthAnalysis {
        manifest: PathBuf,
        #[arg(long)]
        experimental: bool,
    },
    /// 正解ラベル付きサンプルからカテゴリ別の検出率と誤検知率を出す
    Labeled { manifest: PathBuf },
    /// 生成文書に付けたスロップの注釈（意味破綻型・過剰圧縮型）と指摘の重なりを測る
    Spans {
        annotations: PathBuf,
        #[arg(long)]
        experimental: bool,
        /// 集計から外す規則（カンマ区切り）。注釈と偶然重なるだけの規則を外して見る
        #[arg(long, value_delimiter = ',')]
        ignore: Vec<String>,
        /// 文書idの正規表現。調整に使った文書と評価用の文書を分けて測る
        #[arg(long)]
        documents: Option<String>,
        /// lintの指摘を使わず、LLMの指摘（suiko meaning）だけを測る
        #[arg(long, conflicts_with = "linter")]
        no_lint: bool,
        /// suikoの代わりに照合する他のツールのlint。出力は--linter-cacheに文書idごとに保存する
        #[arg(long, value_enum)]
        linter: Option<evaluation::ExternalTool>,
        /// 他のツールのlintの出力（<文書id>.json）を保存・再利用するディレクトリ
        #[arg(long, requires = "linter")]
        linter_cache: Option<PathBuf>,
        /// 他のツールのlintのスクリプト。キャッシュにない文書だけ実行する
        #[arg(long, requires = "linter")]
        linter_script: Option<PathBuf>,
        /// 注釈と指摘を行単位に広げて照合する（行しか返さないツールと同じ粒度で比べる）
        #[arg(long)]
        lines: bool,
        /// 読解負荷の指摘（suikoとnatural-japaneseの両方）も数える
        #[arg(long)]
        reading_load: bool,
        /// suiko meaningの返答を文書idごとに保存・再利用するディレクトリ
        #[arg(long)]
        meaning_cache: Option<PathBuf>,
        /// suiko meaningに渡すLLMのコマンド（`--` の後に指定）。キャッシュにない文書だけ実行する
        #[arg(last = true, value_name = "COMMAND")]
        meaning_command: Vec<String>,
    },
    /// 他のツールの指摘のうちsuikoの指摘と同じ箇所に当たるものと、その逆を規則ごとに数える
    Coverage {
        /// 文書の一覧。既定はspansと同じ注釈ファイル（注釈そのものは使わない）、--humanならcorpus.toml
        documents_file: PathBuf,
        /// documents_fileをcorpus.tomlとして読み、人間の文書で比べる
        #[arg(long)]
        human: bool,
        /// --humanのとき、sources.tomlの外部取得文書(ローカル、lock一致分)も含める
        #[arg(long, requires = "human")]
        external: bool,
        #[arg(long, value_enum)]
        linter: evaluation::ExternalTool,
        /// 他のツールのlintの出力（<文書id>.json）を保存・再利用するディレクトリ
        #[arg(long)]
        linter_cache: PathBuf,
        /// 他のツールのlintのスクリプト。キャッシュにない文書だけ実行する
        #[arg(long)]
        linter_script: Option<PathBuf>,
        /// 他のツールを実験的な規則込みで実行した出力を使う（natural-japaneseだけ）
        #[arg(long)]
        experimental: bool,
        /// 文書idの正規表現
        #[arg(long)]
        documents: Option<String>,
        /// suikoの指摘と重ならなかった指摘の本文を出す規則（カンマ区切り）
        #[arg(long, value_delimiter = ',')]
        show: Vec<String>,
        /// suikoの指摘を前後の文脈つきで出す規則（カンマ区切り）。人の文書の出力はコミットしない
        #[arg(long, value_delimiter = ',')]
        show_suiko: Vec<String>,
        /// --humanのとき、調整用（dev）か評価用（holdout）の文書だけで比べる
        #[arg(long, value_enum, requires = "human")]
        split: Option<SplitFilter>,
    },
    /// 規則ごとの品質表と、既定・実験の扱いの判定（降格候補・昇格候補・正例不足）を出す
    Quality {
        manifest: PathBuf,
        /// sources.tomlの外部取得文書(ローカル、lock一致分)を含める
        #[arg(long)]
        external: bool,
        /// 人間文書に付く割合のWilson 95%上限の許容値
        #[arg(long, default_value_t = evaluation::DEFAULT_MAX_HUMAN_FIRE_UPPER)]
        max_human_fire_upper: f64,
    },
    /// 人間fprのWilson上限を制約に閾値を探索し、推奨値を出す(dev splitのみ)
    Calibrate {
        manifest: PathBuf,
        #[arg(long, value_enum)]
        rule: SweepRule,
        #[arg(
            long,
            value_delimiter = ',',
            required = true,
            allow_hyphen_values = true
        )]
        values: Vec<f64>,
        /// 人間文書fprのWilson 95%上限の許容値(必須。実行前に決めて記録する)
        #[arg(long)]
        max_human_fpr_upper: f64,
        /// sources.tomlの外部取得文書(ローカル、lock一致分)を含める
        #[arg(long)]
        external: bool,
        /// このid接頭辞の文書を除外する(例: 時代の異なる aozora-)
        #[arg(long)]
        exclude_id_prefix: Option<String>,
        #[arg(long)]
        experimental: bool,
    },
    /// 禁止語・誇張語彙の人間/AI出現実測と、AI側に偏る内容語の候補を出す
    Vocab {
        manifest: PathBuf,
        /// sources.tomlの外部取得文書(ローカル、lock一致分)を含める
        #[arg(long)]
        external: bool,
        /// このid接頭辞の文書を除外する(例: 時代の異なる aozora-)
        #[arg(long)]
        exclude_id_prefix: Option<String>,
    },
}

fn execute(cli: Cli) -> Result<String, evaluation::EvaluationError> {
    match cli.command {
        Command::Feedback { decisions } => evaluation::feedback(&decisions),
        Command::Fetch { sources, id, limit } => {
            evaluation::fetch_corpus(&sources, id.as_deref(), limit)
        }
        Command::Report {
            manifest,
            experimental,
            split,
            external,
        } => evaluation::report(&manifest, experimental, split, external),
        Command::Sweep {
            manifest,
            rule,
            values,
            experimental,
        } => evaluation::sweep(&manifest, rule, &values, experimental),
        Command::LengthAnalysis {
            manifest,
            experimental,
        } => evaluation::length_analysis(&manifest, experimental),
        Command::Labeled { manifest } => evaluation::labeled(&manifest),
        Command::Spans {
            annotations,
            experimental,
            ignore,
            documents,
            no_lint,
            linter,
            linter_cache,
            linter_script,
            lines,
            reading_load,
            meaning_cache,
            meaning_command,
        } => {
            let enabled = meaning_cache.is_some() || !meaning_command.is_empty();
            let meaning = enabled.then_some(evaluation::MeaningSource {
                cache: meaning_cache,
                command: meaning_command,
            });
            if no_lint && meaning.is_none() {
                return Err(evaluation::EvaluationError::Invalid(
                    "--no-lintには--meaning-cacheか`--`の後のコマンドが必要です".to_owned(),
                ));
            }
            let linter = match (linter, linter_cache) {
                (Some(tool), Some(cache)) => {
                    evaluation::Linter::External(evaluation::ExternalLint {
                        tool,
                        cache,
                        script: linter_script,
                    })
                }
                (Some(_), None) => {
                    return Err(evaluation::EvaluationError::Invalid(
                        "--linterには出力を保存する--linter-cacheが必要です".to_owned(),
                    ));
                }
                (None, _) if no_lint => evaluation::Linter::Off,
                (None, _) => evaluation::Linter::Suiko,
            };
            evaluation::spans(
                &annotations,
                &evaluation::SpansOptions {
                    experimental,
                    ignore,
                    documents,
                    linter,
                    meaning,
                    lines,
                    reading_load,
                },
            )
        }
        Command::Coverage {
            documents_file,
            human,
            external,
            linter,
            linter_cache,
            linter_script,
            experimental,
            documents,
            show,
            show_suiko,
            split,
        } => evaluation::coverage(&evaluation::CoverageOptions {
            documents: if human {
                evaluation::CoverageDocuments::Human {
                    manifest: documents_file,
                    external,
                    split,
                }
            } else {
                evaluation::CoverageDocuments::Annotations(documents_file)
            },
            external: evaluation::ExternalLint {
                tool: linter,
                cache: linter_cache,
                script: linter_script,
            },
            experimental,
            filter: documents,
            show,
            show_suiko,
        }),
        Command::Quality {
            manifest,
            external,
            max_human_fire_upper,
        } => evaluation::quality(&manifest, external, max_human_fire_upper),
        Command::Calibrate {
            manifest,
            rule,
            values,
            max_human_fpr_upper,
            external,
            exclude_id_prefix,
            experimental,
        } => evaluation::calibrate(
            &manifest,
            rule,
            &values,
            max_human_fpr_upper,
            external,
            exclude_id_prefix.as_deref(),
            experimental,
        ),
        Command::Vocab {
            manifest,
            external,
            exclude_id_prefix,
        } => evaluation::vocab(&manifest, external, exclude_id_prefix.as_deref()),
    }
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("エラー: {error}");
            ExitCode::from(1)
        }
    }
}
