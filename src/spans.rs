//! 生成した文書に人やモデルが付けたスロップの注釈と、suikoの指摘の重なりを測る。
//! 注釈はtodeskingの分類（意味破綻型・過剰圧縮型）に従う。注釈は網羅的とは限らないため、
//! 注釈と重ならない指摘は誤検知と同じではない。

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

use serde::Deserialize;
use suiko::lint::{self, Span};
use suiko::morphology::Morphology;

use super::support::{read, read_toml, utf8};
use super::{EvaluationError, rate_part};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpanSet {
    schema_version: u32,
    generated: String,
    annotator: String,
    #[serde(default)]
    note: String,
    document: Vec<SpanDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpanDocument {
    id: String,
    /// 注釈ファイルからの相対パス。
    path: String,
    model: String,
    style: String,
    /// suikoに渡すジャンル（essay・tech・business）。
    #[serde(default)]
    genre: Option<String>,
    #[serde(default)]
    span: Vec<AnnotatedSpan>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotatedSpan {
    /// 本文に一度だけ現れる部分文字列。
    text: String,
    kind: Kind,
    /// 過剰圧縮型の下位分類（省きすぎ・動詞の独自用法・主題と述語の不一致）など。
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    review: Review,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Kind {
    SemanticBreakdown,
    OverCompression,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SemanticBreakdown => "semantic_breakdown",
            Self::OverCompression => "over_compression",
        }
    }
}

/// 人による確認。rejectedは集計から外す。
#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Review {
    #[default]
    Unreviewed,
    Accepted,
    Rejected,
}

#[derive(Default)]
struct Tally {
    hit: usize,
    total: usize,
}

impl Tally {
    fn add(&mut self, hit: bool) {
        self.total += 1;
        self.hit += usize::from(hit);
    }
}

#[derive(Default)]
struct CategoryCount {
    findings: usize,
    overlapping: usize,
    spans_hit: usize,
}

#[derive(Default)]
struct StyleCount {
    documents: usize,
    chars: usize,
    spans: usize,
    findings: usize,
}

fn line_starts(body: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(body.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}

fn absolute(starts: &[usize], span: &Span) -> Option<Range<usize>> {
    let start = starts.get(span.start_line.checked_sub(1)?)? + span.start_byte;
    let end = starts.get(span.end_line.checked_sub(1)?)? + span.end_byte;
    (start < end).then_some(start..end)
}

fn locate(id: &str, body: &str, text: &str) -> Result<Range<usize>, EvaluationError> {
    let mut found = body.match_indices(text).map(|(index, _)| index);
    match (found.next(), found.next()) {
        (Some(start), None) if !text.is_empty() => Ok(start..start + text.len()),
        (None, _) => Err(EvaluationError::Invalid(format!(
            "{id}: 注釈の文字列が本文にありません: {text}"
        ))),
        _ => Err(EvaluationError::Invalid(format!(
            "{id}: 注釈の文字列が本文に一度だけ現れるよう長くしてください: {text}"
        ))),
    }
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn tally_lines(output: &mut String, label: &str, tallies: &BTreeMap<String, Tally>) {
    for (key, tally) in tallies {
        output.push_str(&format!(
            "{label}={key}\tdetected={}/{}\t{}\n",
            tally.hit,
            tally.total,
            rate_part("detection", tally.hit, tally.total)
        ));
    }
}

/// `ignore`の規則は集計から外す。「〜は、」の読点のように、注釈と偶然重なるだけで
/// 意味の破綻を指していない規則を外した検出率を見るために使う。
/// `documents`はidの正規表現で、規則の調整に使った文書と評価用の文書を分けて測るために使う。
pub fn spans(
    path: &Path,
    experimental: bool,
    ignore: &[String],
    documents: Option<&str>,
) -> Result<String, EvaluationError> {
    let mut set: SpanSet = read_toml(path)?;
    if set.schema_version != 1 {
        return Err(EvaluationError::Invalid(
            "注釈はschema_version = 1を指定してください".to_owned(),
        ));
    }
    if let Some(pattern) = documents {
        let filter = regex::Regex::new(pattern).map_err(|error| {
            EvaluationError::Invalid(format!("--documentsの正規表現が不正です: {error}"))
        })?;
        set.document
            .retain(|document| filter.is_match(&document.id));
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let morphology = Morphology::new()?;

    let mut overall = Tally::default();
    let mut by_kind = BTreeMap::<String, Tally>::new();
    let mut by_model = BTreeMap::<String, Tally>::new();
    let mut by_style = BTreeMap::<String, Tally>::new();
    let mut categories = BTreeMap::<String, CategoryCount>::new();
    let mut styles = BTreeMap::<String, StyleCount>::new();
    let (mut spanned, mut overlapping, mut unspanned, mut rejected, mut accepted) = (0, 0, 0, 0, 0);
    let mut details = String::new();

    for document in &set.document {
        let file = base.join(&document.path);
        let body = utf8(&file, read(&file)?)?;
        let report = lint::analyze(&body, &morphology, document.genre.as_deref(), experimental)?;
        let starts = line_starts(&body);
        let kept = report
            .findings
            .iter()
            .filter(|finding| !ignore.contains(&finding.category))
            .collect::<Vec<_>>();
        let findings = kept
            .iter()
            .filter_map(|finding| {
                let range = absolute(&starts, finding.span.as_ref()?)?;
                Some((finding.category.as_str(), range))
            })
            .collect::<Vec<_>>();
        unspanned += kept.len() - findings.len();

        let mut annotated = Vec::new();
        for span in &document.span {
            let range = locate(&document.id, &body, &span.text)?;
            match span.review {
                Review::Rejected => rejected += 1,
                Review::Accepted => {
                    accepted += 1;
                    annotated.push((span, range));
                }
                Review::Unreviewed => annotated.push((span, range)),
            }
        }

        let style = styles.entry(document.style.clone()).or_default();
        style.documents += 1;
        style.chars += body.chars().count();
        style.spans += annotated.len();
        style.findings += kept.len();

        for (category, range) in &findings {
            let count = categories.entry((*category).to_owned()).or_default();
            count.findings += 1;
            if annotated.iter().any(|(_, span)| overlaps(span, range)) {
                count.overlapping += 1;
                overlapping += 1;
            }
            spanned += 1;
        }
        for (span, range) in &annotated {
            let mut hits = findings
                .iter()
                .filter(|(_, finding)| overlaps(finding, range))
                .map(|(category, _)| *category)
                .collect::<Vec<_>>();
            hits.sort_unstable();
            hits.dedup();
            for category in &hits {
                categories
                    .get_mut(*category)
                    .expect("hit category is counted")
                    .spans_hit += 1;
            }
            let hit = !hits.is_empty();
            overall.add(hit);
            by_kind
                .entry(span.kind.as_str().to_owned())
                .or_default()
                .add(hit);
            by_model.entry(document.model.clone()).or_default().add(hit);
            by_style.entry(document.style.clone()).or_default().add(hit);
            details.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                if hit { "hit" } else { "miss" },
                document.id,
                span.kind.as_str(),
                span.pattern.as_deref().unwrap_or("-"),
                if hit { hits.join(",") } else { "-".to_owned() },
                span.text.replace('\n', "⏎"),
                span.reason.as_deref().unwrap_or("-"),
            ));
        }
    }

    let mut output = format!(
        "slop spans: documents={} generated={} annotator={}\n",
        set.document.len(),
        set.generated,
        set.annotator
    );
    if !set.note.is_empty() {
        output.push_str(&format!("note: {}\n", set.note));
    }
    output.push_str(&format!(
        "mode={} spans={} accepted={accepted} unreviewed={} rejected_excluded={rejected}\n",
        if experimental {
            "experimental"
        } else {
            "default"
        },
        overall.total,
        overall.total - accepted,
    ));
    if let Some(pattern) = documents {
        output.push_str(&format!("documents_filter={pattern}\n"));
    }
    if !ignore.is_empty() {
        output.push_str(&format!("ignored_categories={}\n", ignore.join(",")));
    }
    output.push_str(&format!(
        "overall\tdetected={}/{}\t{}\n",
        overall.hit,
        overall.total,
        rate_part("detection", overall.hit, overall.total)
    ));
    tally_lines(&mut output, "kind", &by_kind);
    tally_lines(&mut output, "model", &by_model);
    tally_lines(&mut output, "style", &by_style);
    output.push_str(&format!(
        "findings: with_span={spanned} overlapping_annotation={overlapping} {} without_span={unspanned}\n",
        rate_part("overlap", overlapping, spanned)
    ));
    output.push_str(
        "注釈と重ならない指摘は、注釈が網羅的でない限り誤検知とは言えない（別の不自然さ・読解負荷の候補を含む）\n",
    );
    output.push_str("style\tdocuments\tchars\tspans_per_1k\tfindings_per_1k\n");
    for (name, style) in &styles {
        let per_1k = |count: usize| count as f64 * 1000.0 / style.chars.max(1) as f64;
        output.push_str(&format!(
            "style={name}\t{}\t{}\t{:.2}\t{:.2}\n",
            style.documents,
            style.chars,
            per_1k(style.spans),
            per_1k(style.findings)
        ));
    }
    output.push_str("category\tfindings\toverlapping\tspans_hit\n");
    let mut ordered = categories.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .1
            .spans_hit
            .cmp(&left.1.spans_hit)
            .then(right.1.findings.cmp(&left.1.findings))
            .then(left.0.cmp(right.0))
    });
    for (category, count) in ordered {
        output.push_str(&format!(
            "category={category}\t{}\t{}\t{}\n",
            count.findings, count.overlapping, count.spans_hit
        ));
    }
    output.push_str("result\tdocument\tkind\tpattern\tcategories\ttext\treason\n");
    output.push_str(&details);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_map_line_relative_bytes_and_require_unique_text() {
        let body = "一行目\n二行目の文\n";
        let starts = line_starts(body);
        let span = Span {
            start_line: 2,
            start_column: 1,
            end_line: 2,
            end_column: 3,
            start_byte: 0,
            end_byte: "二行".len(),
        };
        let range = absolute(&starts, &span).unwrap();
        assert_eq!(&body[range.clone()], "二行");
        assert!(overlaps(&range, &locate("d", body, "行目の").unwrap()));
        assert!(!overlaps(&range, &locate("d", body, "一行目").unwrap()));
        assert!(locate("d", body, "行目").is_err());
        assert!(locate("d", body, "三行目").is_err());
    }
}
