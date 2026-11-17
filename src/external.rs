//! 他のツール（natural-japanese・yomiyasu）のlintの出力を読み、本文上の範囲へ対応づける。
//! 出力は文書idごとのJSONとしてキャッシュに保存し、キャッシュにない文書だけスクリプトを実行する。
//! どちらも列を返さないため、範囲は抜き出しの文字列から本文を探して決める。

use std::io::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use clap::ValueEnum;
use regex::Regex;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::EvaluationError;
use super::support::{read, utf8};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ExternalTool {
    /// coji/natural-japaneseの`scripts/lint.py`（`uv run`で実行する）
    NaturalJapanese,
    /// nanaism/yomiyasuの`scripts/yomiyasu_lint.py`（`python3`で実行する）
    Yomiyasu,
}

impl ExternalTool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NaturalJapanese => "natural-japanese",
            Self::Yomiyasu => "yomiyasu",
        }
    }
}

/// 他のツールのlintの出力の取り方。
pub struct ExternalLint {
    pub tool: ExternalTool,
    /// 文書idごとの出力（`<id>.json`）を保存・再利用するディレクトリ。
    pub cache: PathBuf,
    /// ツールのスクリプト。キャッシュにない文書だけ実行する。
    pub script: Option<PathBuf>,
}

/// 指摘の規則名と本文上の範囲。範囲を決められない指摘（文書全体の統計など）はNone。
pub(super) type Located = (String, Option<Vec<Range<usize>>>);

pub(super) struct Document<'a> {
    pub id: &'a str,
    /// 注釈ファイルのディレクトリ。natural-japaneseは出力にファイルのパスを残すため、
    /// ここを作業ディレクトリにして注釈ファイルからの相対パスで実行する。
    pub base: &'a Path,
    pub path: &'a Path,
    pub genre: Option<&'a str>,
    pub body: &'a str,
    pub starts: &'a [usize],
}

/// 範囲を抜き出しの文字列で決められず、行全体にした指摘の数を併せて返す。
pub(super) struct ExternalFindings {
    pub findings: Vec<Located>,
    pub whole_line: usize,
}

/// yomiyasuの`--doc-type`。tech・businessは除外する規則のないarticle（proposal・formalと同じ規則）にする。
fn doc_type(genre: Option<&str>) -> Option<&'static str> {
    match genre? {
        "essay" => Some("essay"),
        "tech" | "business" => Some("article"),
        _ => None,
    }
}

impl ExternalLint {
    /// `reading_load`がfalseなら、natural-japaneseの読解負荷の指摘を除く。yomiyasuには読解負荷の区分がない。
    pub(super) fn findings(
        &self,
        document: &Document<'_>,
        experimental: bool,
        reading_load: bool,
    ) -> Result<ExternalFindings, EvaluationError> {
        let raw = self.output(document, experimental)?;
        match self.tool {
            ExternalTool::NaturalJapanese => {
                natural_japanese(&raw, document, experimental, reading_load)
            }
            ExternalTool::Yomiyasu => yomiyasu(&raw, document),
        }
    }

    fn output(
        &self,
        document: &Document<'_>,
        experimental: bool,
    ) -> Result<String, EvaluationError> {
        let cached = self.cache.join(format!("{}.json", document.id));
        if cached.exists() {
            return utf8(&cached, read(&cached)?);
        }
        let tool = self.tool.as_str();
        let script = self.script.as_ref().ok_or_else(|| {
            EvaluationError::Invalid(format!(
                "{}: {tool}の出力のキャッシュがなく、--linter-scriptも指定されていません",
                document.id
            ))
        })?;
        let script = std::fs::canonicalize(script).map_err(|source| EvaluationError::Read {
            path: script.display().to_string(),
            source,
        })?;
        let mut command = match self.tool {
            ExternalTool::NaturalJapanese => {
                let mut command = Command::new("uv");
                command
                    .arg("run")
                    .arg(&script)
                    .arg(document.path)
                    .args(["--json", "--reading-load"])
                    .current_dir(document.base)
                    .stdin(Stdio::null());
                if let Some(genre) = document.genre {
                    command.args(["--genre", genre]);
                }
                if experimental {
                    command.arg("--experimental");
                }
                command
            }
            ExternalTool::Yomiyasu => {
                // yomiyasuは作業ディレクトリの外のファイルを読まないため、本文は標準入力で渡す。
                let mut command = Command::new("python3");
                command
                    .arg(&script)
                    .args(["--json", "--no-words"])
                    .stdin(Stdio::piped());
                if let Some(doc_type) = doc_type(document.genre) {
                    command.args(["--doc-type", doc_type]);
                }
                command.arg("-");
                command
            }
        };
        let failed = |message: String| {
            EvaluationError::Invalid(format!(
                "{}: {tool}を実行できません: {message}",
                document.id
            ))
        };
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| failed(error.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(document.body.as_bytes())
                .map_err(|error| failed(error.to_string()))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| failed(error.to_string()))?;
        if !output.status.success() {
            return Err(failed(String::from_utf8_lossy(&output.stderr).into_owned()));
        }
        let raw = String::from_utf8(output.stdout)
            .map_err(|_| failed("出力がUTF-8ではありません".to_owned()))?;
        let write = |source| EvaluationError::Write {
            path: cached.display().to_string(),
            source,
        };
        std::fs::create_dir_all(&self.cache).map_err(write)?;
        std::fs::write(&cached, &raw).map_err(write)?;
        Ok(raw)
    }
}

fn parse<T: DeserializeOwned>(id: &str, raw: &str) -> Result<T, EvaluationError> {
    serde_json::from_str(raw).map_err(|error| EvaluationError::Parse {
        path: format!("{id}.json"),
        message: error.to_string(),
    })
}

/// 改行を含まない行の範囲（1始まりの行番号）。
fn line_range(body: &str, starts: &[usize], line: usize) -> Option<Range<usize>> {
    let start = *starts.get(line.checked_sub(1)?)?;
    let end = starts.get(line).map_or(body.len(), |next| next - 1);
    Some(start..end)
}

/// 行の中から始まる抜き出しの範囲。抜き出しが統計の文字列なら本文になくNoneになる。
fn excerpt_range(body: &str, starts: &[usize], line: usize, excerpt: &str) -> Option<Range<usize>> {
    let excerpt = excerpt.trim();
    if excerpt.is_empty() {
        return None;
    }
    let line = line_range(body, starts, line)?;
    let start = line.start + body[line.start..].find(excerpt)?;
    (start < line.end).then_some(start..start + excerpt.len())
}

#[derive(Deserialize)]
struct NaturalJapaneseOutput {
    stats: NaturalJapaneseStats,
    findings: Vec<NaturalJapaneseFinding>,
    reading_load: Option<NaturalJapaneseSection>,
}

#[derive(Deserialize)]
struct NaturalJapaneseStats {
    genre: Option<String>,
    experimental: bool,
}

#[derive(Deserialize)]
struct NaturalJapaneseSection {
    findings: Vec<NaturalJapaneseFinding>,
}

#[derive(Deserialize)]
struct NaturalJapaneseFinding {
    line: usize,
    category: String,
    excerpt: String,
}

/// 出力は常に`--reading-load`付きで取り、読解負荷の指摘を数えるかは`reading_load`で選ぶ。
fn natural_japanese(
    raw: &str,
    document: &Document<'_>,
    experimental: bool,
    reading_load: bool,
) -> Result<ExternalFindings, EvaluationError> {
    let output: NaturalJapaneseOutput = parse(document.id, raw)?;
    if output.stats.experimental != experimental || output.stats.genre.as_deref() != document.genre
    {
        return Err(EvaluationError::Invalid(format!(
            "{}: natural-japaneseの出力の条件（experimental={} genre={}）が指定と違います",
            document.id,
            output.stats.experimental,
            output.stats.genre.as_deref().unwrap_or("-"),
        )));
    }
    let loads = output.reading_load.ok_or_else(|| {
        EvaluationError::Invalid(format!(
            "{}: natural-japaneseの出力に--reading-loadの指摘がありません",
            document.id
        ))
    })?;
    let findings = output
        .findings
        .into_iter()
        .chain(loads.findings.into_iter().filter(|_| reading_load))
        .map(|finding| {
            let range = excerpt_range(
                document.body,
                document.starts,
                finding.line,
                &finding.excerpt,
            );
            (finding.category, range.map(|range| vec![range]))
        })
        .collect();
    Ok(ExternalFindings {
        findings,
        whole_line: 0,
    })
}

#[derive(Deserialize)]
struct YomiyasuOutput {
    format: String,
    format_version: u32,
    context: YomiyasuContext,
    findings: Vec<YomiyasuFinding>,
}

#[derive(Deserialize)]
struct YomiyasuContext {
    doc_type: Option<String>,
}

#[derive(Deserialize)]
struct YomiyasuFinding {
    rule: String,
    line: Option<usize>,
    message: String,
}

fn yomiyasu(raw: &str, document: &Document<'_>) -> Result<ExternalFindings, EvaluationError> {
    let output: YomiyasuOutput = parse(document.id, raw)?;
    if output.format != "yomiyasu-lint" || output.format_version != 2 {
        return Err(EvaluationError::Invalid(format!(
            "{}: yomiyasu-lintの形式2の出力ではありません",
            document.id
        )));
    }
    let expected = doc_type(document.genre);
    if output.context.doc_type.as_deref() != expected {
        return Err(EvaluationError::Invalid(format!(
            "{}: yomiyasuの出力の--doc-type（{}）が指定（{}）と違います",
            document.id,
            output.context.doc_type.as_deref().unwrap_or("-"),
            expected.unwrap_or("-"),
        )));
    }
    let mut whole_line = 0;
    let findings = output
        .findings
        .into_iter()
        .map(|finding| {
            let line = finding
                .line
                .and_then(|line| line_range(document.body, document.starts, line))
                .filter(|line| !line.is_empty());
            let ranges = line.map(|line| {
                let ranges = quoted_ranges(document.body, &line, &finding.message);
                if ranges.is_empty() {
                    whole_line += 1;
                    vec![line]
                } else {
                    ranges
                }
            });
            (finding.rule, ranges)
        })
        .collect();
    Ok(ExternalFindings {
        findings,
        whole_line,
    })
}

/// yomiyasuのメッセージが当たった語を示す形。2文字以上の「」の中身と、直訳語の「当たった言い方: 〜）」。
/// 直し方の例（「〜という点で」など）や構文の型（「AではなくB」）も「」で引くため、行にあるものだけを範囲にする。
static QUOTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("「([^「」]{2,})」|当たった言い方: ([^）]+)）").expect("quote pattern is valid")
});

/// メッセージが示した語の、行の中の出現範囲。
fn quoted_ranges(body: &str, line: &Range<usize>, message: &str) -> Vec<Range<usize>> {
    let text = &body[line.clone()];
    let mut ranges = QUOTE
        .captures_iter(message)
        .flat_map(|captures| match (captures.get(1), captures.get(2)) {
            (Some(quoted), _) => vec![quoted.as_str().trim()],
            // 直訳語は複数の言い方を「、」で並べることがある。
            (None, Some(matched)) => matched.as_str().split('、').map(str::trim).collect(),
            (None, None) => Vec::new(),
        })
        .filter(|quote| !quote.is_empty())
        .flat_map(|quote| {
            text.match_indices(quote)
                .map(|(offset, found)| line.start + offset..line.start + offset + found.len())
                .collect::<Vec<_>>()
        })
        .filter(|range| !range.is_empty())
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| (range.start, range.end));
    ranges.dedup();
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document<'a>(body: &'a str, starts: &'a [usize], genre: Option<&'a str>) -> Document<'a> {
        Document {
            id: "d",
            base: Path::new("."),
            path: Path::new("d.md"),
            genre,
            body,
            starts,
        }
    }

    fn starts(body: &str) -> Vec<usize> {
        std::iter::once(0)
            .chain(body.match_indices('\n').map(|(index, _)| index + 1))
            .collect()
    }

    #[test]
    fn natural_japanese_excerpts_locate_in_their_line_and_statistics_have_no_range() {
        let body = "# 題\n\n雨が降る。雨が降る日は本を読む。\n";
        let starts = starts(body);
        let raw = r#"{
            "stats": {"genre": "essay", "experimental": false},
            "findings": [
                {"line": 3, "category": "repeated_sentence_lead", "excerpt": "雨が降る日"},
                {"line": 1, "category": "low_burstiness", "excerpt": "burstiness=-0.5"},
                {"line": 1, "category": "elsewhere", "excerpt": "雨が降る"}
            ],
            "reading_load": {"findings": [{"line": 3, "category": "kanji_run", "excerpt": "本を読む"}]}
        }"#;
        let found =
            natural_japanese(raw, &document(body, &starts, Some("essay")), false, true).unwrap();
        let without_loads =
            natural_japanese(raw, &document(body, &starts, Some("essay")), false, false).unwrap();
        assert_eq!(without_loads.findings.len(), 3);
        let ranges = found
            .findings
            .iter()
            .map(|(category, ranges)| {
                let text = ranges
                    .as_ref()
                    .map(|ranges| body[ranges[0].clone()].to_owned());
                (category.as_str(), text)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ranges,
            [
                ("repeated_sentence_lead", Some("雨が降る日".to_owned())),
                ("low_burstiness", None),
                // 1行目から探しても、見つかったのが3行目なら1行目の指摘の範囲にしない。
                ("elsewhere", None),
                ("kanji_run", Some("本を読む".to_owned())),
            ]
        );
        assert!(
            natural_japanese(raw, &document(body, &starts, Some("tech")), false, true).is_err()
        );
        assert!(
            natural_japanese(raw, &document(body, &starts, Some("essay")), true, true).is_err()
        );
        let without_reading_load =
            r#"{"stats": {"genre": "essay", "experimental": false}, "findings": []}"#;
        let essay = document(body, &starts, Some("essay"));
        assert!(natural_japanese(without_reading_load, &essay, false, false).is_err());
    }

    #[test]
    fn yomiyasu_findings_use_quoted_words_or_fall_back_to_the_whole_line() {
        let body = "冬の朝は、静かだ。その音が、合図になる。\n二行目。\n";
        let starts = starts(body);
        let raw = r#"{
            "format": "yomiyasu-lint", "format_version": 2,
            "context": {"doc_type": "essay"},
            "findings": [
                {"rule": "pause_comma", "line": 1, "message": "読点があります（「冬の朝は、」「その音が、」）。例:「〜という点で」"},
                {"rule": "sentence_end_repetition", "line": 2, "message": "同じ文末が続きます"},
                {"rule": "calque", "line": 1, "message": "直訳語（型3: 当たった言い方: 合図になる）の候補です"},
                {"rule": "negative_parallelism", "line": 1, "message": "「AではなくB」構文が検出されました"},
                {"rule": "document_level", "line": null, "message": "「冬の朝は、」"}
            ]
        }"#;
        let found = yomiyasu(raw, &document(body, &starts, Some("essay"))).unwrap();
        let texts = found
            .findings
            .iter()
            .map(|(rule, ranges)| {
                let texts = ranges.as_ref().map(|ranges| {
                    ranges
                        .iter()
                        .map(|range| &body[range.clone()])
                        .collect::<Vec<_>>()
                });
                (rule.as_str(), texts)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            [
                ("pause_comma", Some(vec!["冬の朝は、", "その音が、"])),
                ("sentence_end_repetition", Some(vec!["二行目。"])),
                ("calque", Some(vec!["合図になる"])),
                (
                    "negative_parallelism",
                    Some(vec!["冬の朝は、静かだ。その音が、合図になる。"])
                ),
                ("document_level", None),
            ]
        );
        assert_eq!(found.whole_line, 2);
        assert!(yomiyasu(raw, &document(body, &starts, Some("tech"))).is_err());
    }
}
