#[derive(Clone)]
pub struct SearchSource {
    pub aliases: Vec<String>,
    pub weight: f64,
}

impl SearchSource {
    pub fn new_text(text: &str, weight: f64) -> Self {
        SearchSource {
            aliases: vec![text.to_string()],
            weight,
        }
    }
}

pub struct SearchHit {
    pub index: usize,
    pub similarity: f64,
    pub absolute_right: bool,
}

fn search_similarity(source: &str, query: &str) -> f64 {
    if source.is_empty() || query.is_empty() {
        return 0.0;
    }
    let mut source_chars: Vec<char> = source.to_lowercase().replace(' ', "").chars().collect();
    let query_chars: Vec<char> = query.to_lowercase().replace(' ', "").chars().collect();
    let source_len = source_chars.len();
    let query_len = query_chars.len();
    if query_len == 0 {
        return 0.0;
    }
    let mut qp = 0usize;
    let mut len_sum = 0.0f64;
    while qp < query_len {
        let mut sp = 0usize;
        let mut len_max = 0usize;
        let mut sp_max = 0usize;
        let current_source_len = source_chars.len();
        while sp < current_source_len {
            let mut len = 0usize;
            while (qp + len) < query_len
                && (sp + len) < current_source_len
                && source_chars[sp + len] == query_chars[qp + len]
            {
                len += 1;
            }
            if len > len_max {
                len_max = len;
                sp_max = sp;
            }
            sp += if len > 0 { len } else { 1 };
        }
        if len_max > 0 {
            source_chars.drain(sp_max..sp_max + len_max);
            let inc_weight = (1.4f64).powf(3.0 + len_max as f64) - 3.6;
            let dist = (qp as i64 - sp_max as i64).unsigned_abs() as f64;
            let inc_weight = inc_weight * (1.0 + 0.3 * (3.0 - dist).max(0.0));
            len_sum += inc_weight;
        }
        qp += if len_max > 0 { len_max } else { 1 };
    }
    let len_factor = if query_len <= 2 { (3 - query_len) as f64 } else { 1.0 };
    (len_sum / query_len as f64) * (3.0 / (source_len as f64 + 15.0).sqrt()) * len_factor
}

fn search_similarity_weighted(sources: &[SearchSource], query: &str) -> f64 {
    let mut total_weight = 0.0;
    let mut sum = 0.0;
    for src in sources {
        if !src.aliases.is_empty() {
            let best = src.aliases.iter().map(|a| search_similarity(a, query)).fold(0.0f64, f64::max);
            sum += best * src.weight;
        }
        total_weight += src.weight;
    }
    if total_weight == 0.0 {
        0.0
    } else {
        sum / total_weight
    }
}

pub fn search(
    entries_sources: &[Vec<SearchSource>],
    query: &str,
    max_blur_count: usize,
    min_blur_similarity: f64,
) -> Vec<SearchHit> {
    let mut result: Vec<SearchHit> = Vec::new();
    if entries_sources.is_empty() {
        return result;
    }
    let query_parts: Vec<&str> = query.split(' ').collect();
    let mut candidates: Vec<SearchHit> = Vec::new();
    for (i, sources) in entries_sources.iter().enumerate() {
        let similarity = search_similarity_weighted(sources, query);
        let ql = query.to_lowercase();
        let absolute_right = query_parts.iter().all(|qp| {
            let qp_l = qp.to_lowercase();
            sources.iter().any(|src| {
                src.aliases.iter().any(|a| a.replace(' ', "").to_lowercase().contains(&qp_l))
            })
        });
        let _ = ql;
        if absolute_right || similarity >= min_blur_similarity {
            candidates.push(SearchHit { index: i, similarity, absolute_right });
        }
    }
    candidates.sort_by(|l, r| {
        r.absolute_right
            .cmp(&l.absolute_right)
            .then_with(|| r.similarity.partial_cmp(&l.similarity).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut blur_count = 0usize;
    for c in candidates {
        if c.absolute_right {
            result.push(c);
        } else {
            if blur_count == max_blur_count {
                break;
            }
            result.push(c);
            blur_count += 1;
        }
    }
    result
}