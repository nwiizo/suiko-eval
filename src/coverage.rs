//! 他のツールの指摘のうち、suikoの指摘と同じ箇所に当たるものを規則ごとに数える。
//! 他のツールが指す箇所をsuikoも指せるか（その性質をsuikoが持っているか）と、
//! 逆にsuikoだけが指す箇所を、注釈なしで確かめるために使う。
//! 人間の文書で回すと、両者の規則が人の文章にどれだけ付くかを並べて見られる。

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use suiko::lint;
use suiko::morphology::Morphology;

use super::EvaluationError;
use super::SplitFilter;
use super::external::{self, ExternalLint, ExternalTool, Located};
use super::manifest::{Label, load_corpus, load_external_documents};
use super::spans::{line_starts, load_set, overlaps, suiko_located};
use super::support::{read, utf8};

/// 比べる文書。
pub enum CoverageDocuments {
    /// spansと同じ注釈ファイルの文書。注釈そのものは使わない。
    Annotations(PathBuf),
    /// corpus.tomlの人間の文書。`external`なら取得済みの外部文書も含める。
    /// `split`を指定すると、調整用（dev）か評価用（holdout）の文書だけにする。
    Human {
        manifest: PathBuf,
        external: bool,
        split: Option<SplitFilter>,
    },
}

pub struct CoverageOptions {
    pub documents: CoverageDocuments,
    pub external: ExternalLint,
    /// 他のツールを実験的な規則込みで実行した出力を使う（natural-japaneseだけ）。
    pub experimental: bool,
    /// 文書idの正規表現。
    pub filter: Option<String>,
    /// suiko（実験的な規則込み）の指摘と重ならなかった、他のツールの指摘の本文を出す規則。
    pub show: Vec<String>,
    /// suiko（実験的な規則込み）の指摘を、前後の文脈と同じ箇所を指した他のツールの規則つきで出す規則。
    /// 規則の条件を決めるとき、人の文書に付いた指摘を読むために使う。
    pub show_suiko: Vec<String>,
}

struct Item {
    id: String,
    base: PathBuf,
    path: PathBuf,
    genre: Option<String>,
    body: String,
}

fn items(
    documents: &CoverageDocuments,
    filter: Option<&str>,
) -> Result<Vec<Item>, EvaluationError> {
    let mut items = match documents {
        CoverageDocuments::Annotations(path) => {
            let base = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            let mut items = Vec::new();
            for document in load_set(path, None)?.document {
                let file = base.join(&document.path);
                items.push(Item {
                    id: document.id,
                    body: utf8(&file, read(&file)?)?,
                    base: base.clone(),
                    path: PathBuf::from(document.path),
                    genre: document.genre,
                });
            }
            items
        }
        CoverageDocuments::Human {
            manifest,
            external,
            split,
        } => {
            let base = manifest
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            let mut documents = load_corpus(manifest)?.documents;
            if *external {
                documents.extend(load_external_documents(manifest)?.0);
            }
            documents
                .into_iter()
                .filter(|document| {
                    document.label == Label::Human
                        && split.is_none_or(|split| document.split == split.as_split())
                })
                .map(|document| Item {
                    id: document.id,
                    base: base.clone(),
                    path: document.path,
                    genre: Some(document.genre.as_str().to_owned()),
                    body: document.text,
                })
                .collect()
        }
    };
    if let Some(pattern) = filter {
        let filter = regex::Regex::new(pattern).map_err(|error| {
            EvaluationError::Invalid(format!("--documentsの正規表現が不正です: {error}"))
        })?;
        items.retain(|item| filter.is_match(&item.id));
    }
    Ok(items)
}

/// 明細に出す本文の長さ（文字数）。
const EXCERPT_CHARS: usize = 70;

fn excerpt(body: &str, ranges: &[Range<usize>]) -> String {
    let range = &ranges[0];
    let text = body[range.clone()].replace('\n', "⏎");
    let mut excerpt = text.chars().take(EXCERPT_CHARS).collect::<String>();
    if text.chars().count() > EXCERPT_CHARS {
        excerpt.push('…');
    }
    if ranges.len() > 1 {
        excerpt.push_str(&format!("（ほか{}か所）", ranges.len() - 1));
    }
    excerpt
}

/// 指摘の前後に付ける文脈の長さ（文字数）。
const CONTEXT_CHARS: usize = 25;

/// 指摘の範囲を【】で囲み、同じ行の前後の文脈を付ける。
fn context(body: &str, range: &Range<usize>) -> String {
    let line_start = body[..range.start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = body[range.end..]
        .find('\n')
        .map_or(body.len(), |index| range.end + index);
    let before = body[line_start..range.start]
        .chars()
        .rev()
        .take(CONTEXT_CHARS)
        .collect::<Vec<_>>();
    let after = body[range.end..line_end]
        .chars()
        .take(CONTEXT_CHARS)
        .collect::<String>();
    format!(
        "{}【{}】{after}",
        before.into_iter().rev().collect::<String>(),
        body[range.clone()].replace('\n', "⏎")
    )
}

#[derive(Default)]
struct Count {
    /// その規則が1件以上付いた文書の数。
    documents: usize,
    findings: usize,
    located: usize,
    /// 範囲が行全体の指摘。行の中の位置を示さないため、重なりを甘く数える。
    line_wide: usize,
    /// 比べる相手ごとの、同じ箇所に相手の指摘がある数。
    covered: Vec<usize>,
    by: BTreeMap<String, usize>,
}

/// 範囲のある指摘は範囲の重なりで、範囲のない指摘（文書全体の統計）は同じ文書に同じ規則名があるかで、
/// 相手の指摘が同じ箇所を指しているとみなす。
fn covering<'a>(finding: &Located, others: &'a [Located]) -> Vec<&'a str> {
    let (category, ranges) = finding;
    others
        .iter()
        .filter(
            |(other_category, other_ranges)| match (ranges, other_ranges) {
                (Some(ranges), Some(other_ranges)) => ranges
                    .iter()
                    .any(|range| other_ranges.iter().any(|other| overlaps(range, other))),
                (None, _) => other_category == category,
                (Some(_), None) => false,
            },
        )
        .map(|(other_category, _)| other_category.as_str())
        .collect()
}

fn is_line_wide(body: &str, starts: &[usize], ranges: &[Range<usize>]) -> bool {
    let [range] = ranges else {
        return false;
    };
    let index = starts.partition_point(|start| *start <= range.start) - 1;
    let end = starts.get(index + 1).map_or(body.len(), |next| next - 1);
    range.start == starts[index] && range.end == end
}

fn tally(
    counts: &mut BTreeMap<String, Count>,
    body: &str,
    starts: &[usize],
    findings: &[Located],
    others: &[&[Located]],
) {
    let mut fired = BTreeSet::new();
    for finding in findings {
        let count = counts.entry(finding.0.clone()).or_default();
        count.covered.resize(others.len(), 0);
        count.findings += 1;
        if fired.insert(finding.0.as_str()) {
            count.documents += 1;
        }
        if let Some(ranges) = &finding.1 {
            count.located += 1;
            count.line_wide += usize::from(is_line_wide(body, starts, ranges));
        }
        for (index, other) in others.iter().enumerate() {
            let mut by = covering(finding, other);
            by.sort_unstable();
            by.dedup();
            count.covered[index] += usize::from(!by.is_empty());
            // 規則の対応は、最も広い比較（最後の相手）で記録する。
            if index + 1 == others.len() {
                for category in by {
                    *count.by.entry(category.to_owned()).or_default() += 1;
                }
            }
        }
    }
}

fn table(output: &mut String, header: &[&str], counts: &BTreeMap<String, Count>) {
    output.push_str(&format!(
        "category\tdocuments\tfindings\tlocated\tline_wide\t{}\tcovering_categories\n",
        header.join("\t")
    ));
    let mut ordered = counts.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .1
            .findings
            .cmp(&left.1.findings)
            .then(left.0.cmp(right.0))
    });
    for (category, count) in ordered {
        let mut by = count.by.iter().collect::<Vec<_>>();
        by.sort_by(|left, right| right.1.cmp(left.1).then(left.0.cmp(right.0)));
        let by = by
            .iter()
            .take(4)
            .map(|(name, count)| format!("{name}:{count}"))
            .collect::<Vec<_>>();
        let covered = count
            .covered
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        output.push_str(&format!(
            "category={category}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            count.documents,
            count.findings,
            count.located,
            count.line_wide,
            covered.join("\t"),
            if by.is_empty() {
                "-".to_owned()
            } else {
                by.join(",")
            },
        ));
    }
}

pub fn coverage(options: &CoverageOptions) -> Result<String, EvaluationError> {
    let lint_tool = &options.external;
    if options.experimental && lint_tool.tool == ExternalTool::Yomiyasu {
        return Err(EvaluationError::Invalid(
            "yomiyasuには実験的な規則の指定がないため、--experimentalは使えません".to_owned(),
        ));
    }
    let items = items(&options.documents, options.filter.as_deref())?;
    let morphology = Morphology::new()?;
    let mut theirs = BTreeMap::<String, Count>::new();
    let mut ours = BTreeMap::<String, Count>::new();
    let mut chars = 0;
    let mut uncovered = String::new();
    let mut listed = String::new();

    for item in &items {
        let body = &item.body;
        chars += body.chars().count();
        let starts = line_starts(body);
        let genre = item.genre.as_deref();
        // suiko lintの既定と同じく、読解負荷の指摘も含める。
        let loads = lint::analyze_reading_load(body, &morphology, genre)?.findings;
        let suiko = |experimental| -> Result<Vec<Located>, EvaluationError> {
            Ok(lint::analyze(body, &morphology, genre, experimental)?
                .findings
                .into_iter()
                .chain(loads.iter().cloned())
                .map(|finding| suiko_located(&starts, finding))
                .collect())
        };
        let (default, experimental) = (suiko(false)?, suiko(true)?);
        let external = lint_tool
            .findings(
                &external::Document {
                    id: &item.id,
                    base: &item.base,
                    path: &item.path,
                    genre,
                    body,
                    starts: &starts,
                },
                options.experimental,
                true,
            )?
            .findings;
        tally(
            &mut theirs,
            body,
            &starts,
            &external,
            &[&default, &experimental],
        );
        tally(&mut ours, body, &starts, &experimental, &[&external]);
        for finding in &experimental {
            let (category, Some(ranges)) = finding else {
                continue;
            };
            if options.show_suiko.contains(category) {
                let mut by = covering(finding, &external);
                by.sort_unstable();
                by.dedup();
                listed.push_str(&format!(
                    "suiko\t{}\t{category}\t{}\t{}\n",
                    item.id,
                    if by.is_empty() {
                        "-".to_owned()
                    } else {
                        by.join(",")
                    },
                    context(body, &ranges[0])
                ));
            }
        }
        for finding in &external {
            let (category, Some(ranges)) = finding else {
                continue;
            };
            if options.show.contains(category) && covering(finding, &experimental).is_empty() {
                uncovered.push_str(&format!(
                    "uncovered\t{}\t{category}\t{}\n",
                    item.id,
                    excerpt(body, ranges)
                ));
            }
        }
    }

    let tool = lint_tool.tool.as_str();
    let source = match &options.documents {
        CoverageDocuments::Annotations(path) => format!("annotations={}", path.display()),
        CoverageDocuments::Human {
            manifest,
            external,
            split,
        } => format!(
            "human={}{}{}",
            manifest.display(),
            if *external { " (external)" } else { "" },
            match split {
                Some(SplitFilter::Dev) => " split=dev",
                Some(SplitFilter::Holdout) => " split=holdout",
                None => "",
            }
        ),
    };
    let mut output = format!(
        "coverage: {source} documents={} chars={chars} linter={tool}{} cache={}\n",
        items.len(),
        if options.experimental {
            " (experimental)"
        } else {
            ""
        },
        lint_tool.cache.display(),
    );
    if let Some(pattern) = &options.filter {
        output.push_str(&format!("documents_filter={pattern}\n"));
    }
    output.push_str(
        "両者とも読解負荷の指摘を含める。範囲のある指摘は範囲の重なり、範囲のない指摘（文書全体の統計）は同じ文書の同じ規則名で、同じ箇所とみなす。line_wideは行全体を範囲にした指摘で、重なりを甘く数える\n",
    );
    output.push_str(&format!(
        "## {tool}の指摘のうち、suikoの指摘と同じ箇所に当たるもの\n"
    ));
    table(
        &mut output,
        &["suiko_default", "suiko_experimental"],
        &theirs,
    );
    output.push_str(&format!(
        "## suiko（実験的な規則込み）の指摘のうち、{tool}の指摘と同じ箇所に当たるもの\n"
    ));
    table(&mut output, &[tool], &ours);
    if !options.show.is_empty() {
        output.push_str(&format!(
            "## suiko（実験的な規則込み）と重ならなかった{tool}の指摘（{}）\n",
            options.show.join(",")
        ));
        output.push_str(&uncovered);
    }
    if !options.show_suiko.is_empty() {
        output.push_str(&format!(
            "## suiko（実験的な規則込み）の指摘（{}）。4列目は同じ箇所を指した{tool}の規則\n",
            options.show_suiko.join(",")
        ));
        output.push_str(&listed);
    }
    Ok(output)
}
