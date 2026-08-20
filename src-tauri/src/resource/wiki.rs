use std::collections::HashMap;
use std::sync::OnceLock;

use super::matcher::{search, SearchHit, SearchSource};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    CurseForge,
    Modrinth,
}

pub struct WikiEntry {
    pub id: i32,
    pub chinese_name: Option<String>,
    pub slug_cf: Option<String>,
    pub slug_mr: Option<String>,
    pub popularity: i32,
}

struct Db {
    entries: Vec<WikiEntry>,
    cf_indices: Vec<usize>,
    mr_indices: Vec<usize>,
    sources_all: HashMap<usize, Vec<Vec<SearchSource>>>,
    cf_by_slug: HashMap<String, usize>,
    mr_by_slug: HashMap<String, usize>,
}

fn radix86_value(ch: char) -> Option<u32> {
    let c = ch as u32;
    if !(33..=126).contains(&c) {
        return None;
    }
    const EXCLUDED: [u32; 8] = [34, 45, 46, 92, 95, 96, 124, 126];
    if EXCLUDED.contains(&c) {
        return None;
    }
    let below = (33..c).filter(|cc| !EXCLUDED.contains(cc)).count() as u32;
    Some(below)
}

fn decode_popularity_code(code: &str) -> i64 {
    let chars: Vec<char> = code.chars().collect();
    if chars.len() != 3 {
        return 0;
    }
    let mut value: i64 = 0;
    for c in &chars {
        match radix86_value(*c) {
            Some(v) => value = value * 86 + v as i64,
            None => return 0,
        }
    }
    value
}

fn after_last<'a>(s: &'a str, needle: &str) -> &'a str {
    match s.rfind(needle) {
        Some(i) => &s[i + needle.len()..],
        None => s,
    }
}

fn before_first<'a>(s: &'a str, needle: &str) -> &'a str {
    match s.find(needle) {
        Some(i) => &s[..i],
        None => s,
    }
}

fn split_simplified(s: &str) -> &str {
    s
}

fn parse_slug_field(field: &str) -> (Option<String>, Option<String>) {
    if let Some(rest) = field.strip_prefix('@') {
        let slug = rest.to_string();
        (None, Some(slug))
    } else if field.ends_with('@') {
        let base = field.trim_end_matches('@').to_string();
        (Some(base.clone()), Some(base))
    } else if field.contains('@') {
        let parts: Vec<&str> = field.split('@').collect();
        let cf = parts.get(0).cloned().unwrap_or("").to_string();
        let mr = parts.get(1).cloned().unwrap_or("").to_string();
        (Some(cf), Some(mr))
    } else {
        let slug = field.to_string();
        (Some(slug), None)
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::new();
            out.push(first.to_uppercase().next().unwrap_or(first));
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

fn build_sources(entry: &WikiEntry, source: Platform) -> Vec<SearchSource> {
    let slug = match source {
        Platform::CurseForge => entry.slug_cf.as_deref(),
        Platform::Modrinth => entry.slug_mr.as_deref(),
    };
    let slug = match slug {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut sources: Vec<SearchSource> = Vec::new();
    if let Some(cn) = &entry.chinese_name {
        let cn_l = split_simplified(cn);
        let aliases_main: Vec<String> = before_first(cn_l, " (")
            .split('/')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if !aliases_main.is_empty() {
            sources.push(SearchSource { aliases: aliases_main, weight: 1.0 });
        }
        let alias_sub = format!("{}{}", split_simplified(after_last(cn_l, " (")), slug);
        sources.push(SearchSource::new_text(&alias_sub, 0.5));
    } else {
        sources.push(SearchSource::new_text(slug, 0.5));
    }
    sources
}

fn db() -> &'static Db {
    static DB: OnceLock<Db> = OnceLock::new();
    DB.get_or_init(|| {
        let raw = include_str!("WikiEntries.txt");
        let mut lines: Vec<&str> = raw.split('\n').collect();
        while lines.last().map_or(false, |l| l.trim().is_empty()) {
            lines.pop();
        }
        let last = lines.pop().unwrap_or("");
        let popularities: Vec<i64> = (0..last.len().saturating_sub(2)).step_by(3)
            .map(|i| decode_popularity_code(&last[i..i + 3]))
            .collect();
        let mut pop_iter = popularities.into_iter();

        let mut entries: Vec<WikiEntry> = Vec::new();
        for (idx, raw_line) in lines.iter().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            let line_number = idx + 1;
            let popularity = pop_iter.next().unwrap_or(0) as i32;
            for item in line.split('\u{00A8}') {
                if item.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = item.split('|').collect();
                let slug_field = parts.get(0).cloned().unwrap_or("");
                let (slug_cf, slug_mr) = parse_slug_field(slug_field);
                let mut chinese_name = if parts.len() >= 2 {
                    let last_part = parts[parts.len() - 1].trim();
                    if last_part.is_empty() {
                        None
                    } else {
                        Some(last_part.to_string())
                    }
                } else {
                    None
                };
                if let Some(cn) = chinese_name.as_mut() {
                    if cn.contains('*') {
                        let base = if slug_cf.is_some() {
                            &slug_cf.clone().unwrap()
                        } else if slug_mr.is_some() {
                            &slug_mr.clone().unwrap()
                        } else {
                            ""
                        };
                        let english = capitalize_first(&base.replace('-', " "));
                        *cn = cn.replace('*', &format!(" ({})", english));
                    }
                }
                entries.push(WikiEntry {
                    id: line_number as i32,
                    chinese_name,
                    slug_cf,
                    slug_mr,
                    popularity,
                });
            }
        }

        let cf_indices: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.slug_cf.is_some())
            .map(|(i, _)| i)
            .collect();
        let mr_indices: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.slug_mr.is_some())
            .map(|(i, _)| i)
            .collect();

        let mut cf_by_slug: HashMap<String, usize> = HashMap::new();
        for &i in &cf_indices {
            if let Some(s) = entries[i].slug_cf.clone() {
                cf_by_slug.insert(s, i);
            }
        }
        let mut mr_by_slug: HashMap<String, usize> = HashMap::new();
        for &i in &mr_indices {
            if let Some(s) = entries[i].slug_mr.clone() {
                mr_by_slug.insert(s, i);
            }
        }

        let mut sources_all: HashMap<usize, Vec<Vec<SearchSource>>> = HashMap::new();
        for idx_list in [&cf_indices, &mr_indices] {
            for &i in idx_list {
                if sources_all.contains_key(&i) {
                    continue;
                }
                let cs = build_sources(&entries[i], Platform::CurseForge);
                let ms = build_sources(&entries[i], Platform::Modrinth);
                sources_all.insert(i, vec![cs, ms]);
            }
        }

        Db { entries, cf_indices, mr_indices, sources_all, cf_by_slug, mr_by_slug }
    })
}

pub fn lookup_entry(source: Platform, slug: &str) -> Option<&'static WikiEntry> {
    let d = db();
    let idx = match source {
        Platform::CurseForge => d.cf_by_slug.get(slug),
        Platform::Modrinth => d.mr_by_slug.get(slug),
    }?;
    let idx = *idx;
    Some(&d.entries[idx])
}

const STOP_WORDS: [&str; 9] = ["the", "of", "mod", "and", "forge", "fabric", "for", "quilt", "neoforge"];

fn can_form(s: &str, local: &[String]) -> bool {
    if local.iter().any(|c| c.as_str() == s) {
        return true;
    }
    local.iter().any(|c| s.starts_with(c.as_str()) && can_form(&s[c.len()..], local))
}

pub fn extract_words(entry: &WikiEntry, source: Platform) -> Vec<String> {
    let slug = match source {
        Platform::CurseForge => entry.slug_cf.as_deref(),
        Platform::Modrinth => entry.slug_mr.as_deref(),
    };
    let mut candidates: Vec<String> = Vec::new();
    if let Some(s) = slug {
        candidates.push(s.replace('-', " ").replace('/', " "));
    }
    if let Some(cn) = &entry.chinese_name {
        let cleaned_after = after_last(cn, " (").trim_end_matches([')', ' ']);
        let cleaned = before_first(cleaned_after, " - ");
        let cleaned = cleaned
            .replace('-', " ")
            .replace('/', " ")
            .replace(':', " ")
            .replace('(', " ")
            .replace(')', "");
        candidates.push(cleaned);
    }

    let mut words: Vec<String> = candidates
        .iter()
        .flat_map(|c| c.split(' '))
        .map(|w| w.trim_start_matches(['{', '[', '(']).trim_end_matches(['}', ']', ')']).to_lowercase())
        .filter(|w| {
            if w.is_empty() || w.chars().count() <= 1 {
                return false;
            }
            if STOP_WORDS.contains(&w.as_str()) {
                return false;
            }
            if let Ok(v) = w.parse::<f64>() {
                if v > 0.0 {
                    return false;
                }
            }
            if !w.is_ascii() {
                return false;
            }
            true
        })
        .collect();
    words.dedup();

    let out: Vec<String> = words
        .iter()
        .filter(|w| {
            !words.iter().any(|c| c.len() < w.chars().count() as usize && w.starts_with(c.as_str()) && can_form(&w[c.len()..], &words))
        })
        .cloned()
        .collect();
    if out.is_empty() {
        words
    } else {
        out
    }
}

pub struct SearchPlan {
    pub is_chinese: bool,
    pub cf_alt: Option<String>,
    pub mr_alt: Option<String>,
    pub mr_slugs: Vec<String>,
}

const MSG_NO_CHINESE_RESULT: &str = "无搜索结果，请尝试搜索其英文名称";

pub fn build_plan(raw_query: &str, res_type: &str, use_cf: bool, use_mr: bool) -> Result<SearchPlan, String> {
    let mut raw = raw_query.trim().to_lowercase();
    raw = split_simplified(&raw).to_string();
    let is_chinese = !raw.is_empty()
        && raw.chars().any(|c| ('\u{4e00}'..='\u{9fbb}').contains(&c))
        && (matches!(res_type, "mod" | "datapack"));

    let plan = SearchPlan { is_chinese, cf_alt: None, mr_alt: None, mr_slugs: Vec::new() };
    if !is_chinese {
        return Ok(plan);
    }

    let d = db();

    let mut cf_alt: Option<String> = None;
    if use_cf {
        let indices = &d.cf_indices;
        let sources: Vec<Vec<SearchSource>> = indices
            .iter()
            .map(|&i| d.sources_all.get(&i).map(|v| v[0].clone()).unwrap_or_default())
            .collect();
        let hits = search(&sources, &raw, 100, 0.25);
        if !hits.is_empty() {
            let target = pick_cf_target(&hits, indices, &d.entries);
            cf_alt = Some(extract_words(target, Platform::CurseForge).join(" "));
        }
    }

    let mut mr_alt: Option<String> = None;
    let mut mr_slugs: Vec<String> = Vec::new();
    if use_mr {
        let indices = &d.mr_indices;
        let sources: Vec<Vec<SearchSource>> = indices
            .iter()
            .map(|&i| d.sources_all.get(&i).map(|v| v[1].clone()).unwrap_or_default())
            .collect();
        let hits = search(&sources, &raw, 100, 0.25);
        if !hits.is_empty() {
            let mut word_weights: HashMap<String, f64> = HashMap::new();
            for hit in &hits {
                let entry = &d.entries[indices[hit.index]];
                for word in extract_words(entry, Platform::Modrinth) {
                    let w = word_weights.entry(word).or_insert(0.0);
                    let exact = d.sources_all[&indices[hit.index]][1]
                        .iter()
                        .any(|s| s.aliases.iter().any(|a| *a == raw));
                    let sim = if exact { 1000.0 } else { hit.similarity };
                    *w += sim * entry.popularity as f64;
                }
            }
            let best = word_weights
                .iter()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(k, _)| k.clone());
            if let Some(alt) = best {
                mr_alt = Some(alt);
            }
            mr_slugs = hits
                .iter()
                .take(100)
                .filter_map(|hit| d.entries[indices[hit.index]].slug_mr.clone())
                .collect();
        }
    }

    if cf_alt.is_none() && mr_alt.is_none() && mr_slugs.is_empty() {
        return Err(MSG_NO_CHINESE_RESULT.to_string());
    }

    Ok(SearchPlan { is_chinese, cf_alt, mr_alt, mr_slugs })
}

fn pick_cf_target<'a>(hits: &[SearchHit], indices: &[usize], entries: &'a [WikiEntry]) -> &'a WikiEntry {
    let first_abs = hits.first().map(|h| h.absolute_right).unwrap_or(false);
    let candidates: Vec<&SearchHit> = if first_abs {
        hits.iter().filter(|h| h.absolute_right).collect()
    } else {
        let max_sim = hits.iter().map(|h| h.similarity).fold(0.0f64, f64::max);
        hits.iter().filter(|h| (h.similarity - max_sim).abs() < 1e-9).collect()
    };
    candidates
        .iter()
        .max_by(|a, b| {
            let pa = entries[indices[a.index]].popularity;
            let pb = entries[indices[b.index]].popularity;
            pa.cmp(&pb)
        })
        .map(|h| &entries[indices[h.index]])
        .unwrap_or(&entries[indices[hits[0].index]])
}