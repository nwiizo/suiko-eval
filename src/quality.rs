//! 規則ごとの品質表と、既定・実験の扱いを決める関門。
//! 人間文書での発火率（雑音の代理指標）とラベル付きサンプルの検出・誤検知を
//! 1行にまとめ、事前に決めた条件で「維持・降格候補・昇格候補・正例不足」を判定する。

use std::collections::BTreeMap;
use std::path::Path;

use suiko::lint;
use suiko::morphology::Morphology;

use super::manifest::{Corpus, load_corpus, load_external_documents};
use super::{
    EvaluationError, LabeledCounts, Lane, SweepThresholds, corpus_line, evaluate, evaluate_samples,
    external_line, label_totals, labeled_counts, wilson_ci,
};

/// 人間文書に付く割合のWilson 95%上限の既定許容値。人間95文書なら7文書（上限0.144）まで通り、
/// 8文書（0.157）で越える。
pub const DEFAULT_MAX_HUMAN_FIRE_UPPER: f64 = 0.15;
/// ラベル付きサンプルの事前登録条件（eval/calibration.md の各規則で使ってきたもの）。
const MIN_FIXTURES: usize = 5;
const MAX_FIXTURE_FPR_UPPER: f64 = 0.30;
const MIN_FIXTURE_DETECTION: f64 = 0.8;

/// 表記・記法・設定の検査。人間の文書に付いても誤検知ではないため、雑音の関門にかけない。
const NOTATION_CATEGORIES: &[&str] = &[
    "auxiliary_kana",
    "bold_not_rendered",
    "custom_wording",
    "duplicate_comma",
    "invalid_ignore_directive",
    "invisible_character",
    "mixed_latin_spacing",
    "word_usage",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Noise {
    /// 人間文書での発火率の上限が許容値以下。
    Quiet,
    /// 人間文書での発火率の上限が許容値を超える。
    Noisy,
    /// 表記・記法の検査で、雑音の関門にかけない。
    Notation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fixtures {
    /// 正例・反例が各5件以上あり、検出率と誤検知の条件を満たす。
    Passed,
    /// 正例・反例のどちらかが5件未満。
    Insufficient,
    /// 件数は足りるが、検出率または誤検知の条件を満たさない。
    Failed,
}

/// 現在の扱い（既定・実験）と二つの判定から、次に取る行動を決める。
pub(crate) fn action(experimental: bool, noise: Noise, fixtures: Fixtures) -> &'static str {
    match (experimental, noise, fixtures) {
        (false, Noise::Noisy, _) => "demotion_candidate",
        (true, Noise::Noisy, _) => "keep_experimental",
        (_, _, Fixtures::Failed) => "fix_rule",
        (_, _, Fixtures::Insufficient) => "add_fixtures",
        (true, _, Fixtures::Passed) => "promotion_candidate",
        (false, _, Fixtures::Passed) => "keep",
    }
}

pub(crate) fn noise(
    category: &str,
    human_fired: usize,
    human_total: usize,
    max_upper: f64,
) -> Noise {
    if NOTATION_CATEGORIES.contains(&category) {
        return Noise::Notation;
    }
    match wilson_ci(human_fired, human_total) {
        Some((_, upper)) if upper <= max_upper => Noise::Quiet,
        _ => Noise::Noisy,
    }
}

pub(crate) fn fixtures(counts: Option<&LabeledCounts>) -> Fixtures {
    let Some(counts) = counts else {
        return Fixtures::Insufficient;
    };
    if counts.fire_total < MIN_FIXTURES || counts.silent_total < MIN_FIXTURES {
        return Fixtures::Insufficient;
    }
    let detection = counts.fire_hit as f64 / counts.fire_total as f64;
    let fpr_upper =
        wilson_ci(counts.silent_fired, counts.silent_total).map_or(1.0, |(_, upper)| upper);
    if detection >= MIN_FIXTURE_DETECTION && fpr_upper <= MAX_FIXTURE_FPR_UPPER {
        Fixtures::Passed
    } else {
        Fixtures::Failed
    }
}

fn noise_label(noise: Noise) -> &'static str {
    match noise {
        Noise::Quiet => "quiet",
        Noise::Noisy => "noisy",
        Noise::Notation => "notation",
    }
}

fn fixtures_label(fixtures: Fixtures) -> &'static str {
    match fixtures {
        Fixtures::Passed => "passed",
        Fixtures::Insufficient => "insufficient",
        Fixtures::Failed => "failed",
    }
}

fn upper(fired: usize, total: usize) -> String {
    wilson_ci(fired, total).map_or_else(|| "-".to_owned(), |(_, upper)| format!("{upper:.3}"))
}

pub fn quality(
    manifest_path: &Path,
    external: bool,
    max_human_fire_upper: f64,
) -> Result<String, EvaluationError> {
    if !(0.0..=1.0).contains(&max_human_fire_upper) {
        return Err(EvaluationError::Invalid(
            "max-human-fire-upperは0以上1以下です".to_owned(),
        ));
    }
    let corpus = load_corpus(manifest_path)?;
    let mut documents = corpus.documents;
    let mut external_stats = None;
    if external {
        let (external_documents, stats) = load_external_documents(manifest_path)?;
        external_stats = Some(stats);
        documents.extend(external_documents);
    }
    let morphology = Morphology::new()?;
    // 実験規則も同じ条件で測るため、常に --experimental 相当で検査する。
    let reports = evaluate(
        &documents,
        &morphology,
        SweepThresholds::default(),
        true,
        None,
    )?;
    let corpus = Corpus {
        manifest_sha256: corpus.manifest_sha256,
        documents,
        samples: corpus.samples,
    };
    let samples = evaluate_samples(&corpus.samples, &morphology)?;
    let labeled = labeled_counts(&samples);
    let (human_total, ai_total) = label_totals(&reports);

    let mut output = format!("documents: human={human_total} ai={ai_total}\n");
    if let Some(stats) = &external_stats {
        output.push_str(&external_line(stats));
    }
    output.push_str(&corpus_line(&corpus));
    output.push_str(&format!(
        "gate: human_fire_upper<={max_human_fire_upper:.3} fixtures>={MIN_FIXTURES}+{MIN_FIXTURES} fixture_detection>={MIN_FIXTURE_DETECTION:.1} fixture_fpr_upper<={MAX_FIXTURE_FPR_UPPER:.2} (wilson95)\n",
    ));
    output.push_str(
        "human_fire is a document-level noise proxy, not precision; ai detection is omitted while ai documents are few\n",
    );
    output.push_str("category\ttier\thuman_fire\thuman_upper\tfixtures_fire\tfixtures_silent\tnoise\tfixtures\taction\n");
    let mut actions = BTreeMap::<&str, Vec<String>>::new();
    for (category, counts) in super::category_counts(reports.iter(), Lane::Naturalness) {
        let experimental = lint::is_experimental_rule(&category);
        let noise = noise(
            &category,
            counts.human_documents,
            human_total,
            max_human_fire_upper,
        );
        let fixture_counts = labeled.get(category.as_str());
        let fixtures = fixtures(fixture_counts);
        let action = action(experimental, noise, fixtures);
        let (fire, silent) = fixture_counts.map_or_else(
            || ("0/0".to_owned(), "0/0".to_owned()),
            |counts| {
                (
                    format!("{}/{}", counts.fire_hit, counts.fire_total),
                    format!("{}/{}", counts.silent_fired, counts.silent_total),
                )
            },
        );
        output.push_str(&format!(
            "{category}\t{}\t{}/{human_total}\t{}\t{fire}\t{silent}\t{}\t{}\t{action}\n",
            if experimental {
                "experimental"
            } else {
                "default"
            },
            counts.human_documents,
            upper(counts.human_documents, human_total),
            noise_label(noise),
            fixtures_label(fixtures),
        ));
        actions.entry(action).or_default().push(category);
    }
    if ai_total > 0 {
        output.push_str(&format!(
            "ai documents: {ai_total} (detection rates are left to `report`; too few to gate on)\n"
        ));
    }
    for (action, categories) in &actions {
        output.push_str(&format!(
            "summary {action}={} {}\n",
            categories.len(),
            categories.join(",")
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(
        fire_hit: usize,
        fire_total: usize,
        silent_fired: usize,
        silent_total: usize,
    ) -> LabeledCounts {
        LabeledCounts {
            fire_total,
            fire_hit,
            silent_total,
            silent_fired,
        }
    }

    #[test]
    fn gate_separates_noise_fixtures_and_tier() {
        assert_eq!(noise("forbidden_phrase", 51, 95, 0.15), Noise::Noisy);
        assert_eq!(noise("short_object_comma", 2, 95, 0.15), Noise::Quiet);
        assert_eq!(noise("auxiliary_kana", 37, 95, 0.15), Noise::Notation);
        // 人間文書が無ければ雑音を測れないので、通さない。
        assert_eq!(noise("thin_evaluation", 0, 0, 0.15), Noise::Noisy);

        assert_eq!(fixtures(None), Fixtures::Insufficient);
        assert_eq!(fixtures(Some(&counts(5, 5, 0, 4))), Fixtures::Insufficient);
        assert_eq!(fixtures(Some(&counts(10, 10, 0, 10))), Fixtures::Passed);
        assert_eq!(fixtures(Some(&counts(3, 5, 0, 10))), Fixtures::Failed);
        assert_eq!(fixtures(Some(&counts(10, 10, 3, 10))), Fixtures::Failed);

        assert_eq!(
            action(false, Noise::Noisy, Fixtures::Passed),
            "demotion_candidate"
        );
        assert_eq!(
            action(true, Noise::Noisy, Fixtures::Passed),
            "keep_experimental"
        );
        assert_eq!(
            action(true, Noise::Quiet, Fixtures::Passed),
            "promotion_candidate"
        );
        assert_eq!(action(false, Noise::Notation, Fixtures::Passed), "keep");
        assert_eq!(
            action(true, Noise::Quiet, Fixtures::Insufficient),
            "add_fixtures"
        );
        assert_eq!(action(false, Noise::Quiet, Fixtures::Failed), "fix_rule");
    }
}
