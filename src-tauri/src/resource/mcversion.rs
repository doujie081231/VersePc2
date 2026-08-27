pub fn name_to_version(name: &str) -> (i32, i32, i32) {
    let mut n = name.to_ascii_lowercase();
    n = n.replace("_unobfuscated", "").replace(" unobfuscated", "");
    if n.starts_with("2.0") {
        return name_to_version("1.5.1");
    }
    if n == "15w14a" {
        return name_to_version("1.8.3");
    }
    if n.contains(".rv-pre") {
        return name_to_version("1.9.2");
    }
    if n.contains("shareware") {
        return name_to_version("1.13.2");
    }
    if n.starts_with("20w14") && n != "20w14a" {
        return name_to_version("1.15.2");
    }
    if n.contains("oneblockatatime") {
        return name_to_version("1.18.2");
    }
    if n.contains("23w13a") && n != "20w13a" {
        return name_to_version("1.19.4");
    }
    if n == "24w14potato" {
        return name_to_version("1.20.4");
    }
    if n == "25w14craftmine" {
        return name_to_version("1.21.4");
    }
    if n == "26w14a" {
        return name_to_version("26.1.1");
    }

    let segs: Vec<&str> = n.split(|c: char| c == ' ' || c == '_' || c == '-' || c == '.').collect();
    let num = |s: &str| s.parse::<i32>().unwrap_or(0);

    if n.starts_with("1.") {
        let major = if segs.len() >= 2 { num(segs[1]) } else { 0 };
        let build = if segs.len() >= 3 { num(segs[2]) } else { 0 };
        return (major, 0, build);
    }

    let b = n.as_bytes();
    if b.len() >= 3 {
        let c0 = b[0];
        if (2..=9).contains(&(c0 - b'0')) && b[1].is_ascii_digit() && b[2] == b'.' {
            let major = (segs.get(0).map(|s| num(s)).unwrap_or(0));
            let minor = if segs.len() >= 2 { num(segs[1]) } else { 0 };
            let build = if segs.len() >= 3 { num(segs[2]) } else { 0 };
            return (major, minor, build);
        }
    }

    (9999, 0, 0)
}

pub fn version_to_drop(name: &str) -> i32 {
    let (major, minor, _) = name_to_version(name);
    if major >= 1000 {
        209
    } else {
        major * 10 + minor
    }
}

pub fn is_format_fit(version: &str) -> bool {
    if version.is_empty() {
        return false;
    }
    let b = version.as_bytes();
    if b.len() >= 2 && b[0] == b'1' && b[1] == b'.' && b.get(2).map_or(false, |c| c.is_ascii_digit()) {
        return true;
    }
    if b.len() >= 3 && (2..=9).contains(&(b[0] - b'0')) && b[1].is_ascii_digit() && b[2] == b'.' {
        let num = (b[0] - b'0') as i32 * 10 + (b[1] - b'0') as i32;
        return num >= 26;
    }
    false
}

pub fn version_snapshot_match(file_version: &str, instance_vanilla: &str) -> bool {
    let file_is_snapshot = file_version.contains("预览版") || !file_version.contains('.');
    let inst_is_snapshot = instance_vanilla.contains("snapshot") || !instance_vanilla.contains('.');
    if file_is_snapshot != inst_is_snapshot {
        return false;
    }
    if !file_is_snapshot && !inst_is_snapshot {
        return file_version == instance_vanilla;
    }
    let file_is_new = file_version.contains('.')
        && file_version
            .split('.')
            .next()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
            > 1.0;
    let inst_is_new = version_to_drop(instance_vanilla) > 250;
    if file_is_new != inst_is_new {
        return false;
    }
    if file_is_new && inst_is_new {
        let f = file_version.split(' ').next().unwrap_or("");
        let i = instance_vanilla.split('-').next().unwrap_or("");
        return f == i;
    }
    if file_version.contains('w') && instance_vanilla.contains('w') {
        return file_version == instance_vanilla;
    }
    true
}

pub fn version_prefix_match(file_version: &str, instance_vanilla: &str) -> bool {
    fn seg(s: &str) -> (i32, i32) {
        let base = s.split(' ').next().unwrap_or("").split('-').next().unwrap_or("");
        let p: Vec<&str> = base.split('.').collect();
        fn num(x: &str) -> i32 {
            x.trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<i32>().unwrap_or(0)
        }
        (num(p.get(0).unwrap_or(&"")), p.get(1).map(|x| num(x)).unwrap_or(0))
    }
    let (fm, fmi) = seg(file_version);
    let (im, imi) = seg(instance_vanilla);
    fm == im && fmi == imi
}

pub fn is_mc_version(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let b = t.as_bytes();
    b[0].is_ascii_digit()
}