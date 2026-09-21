//! Port of `shared/src/codeframe.ts`. Offsets are UTF-16 code units, as in JS.

const RANGE: i64 = 2;

pub fn generate_code_frame(source: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = source.encode_utf16().collect();
    let len = units.len();
    let start = start.min(len);
    let end = end.min(len);
    if start > end {
        return String::new();
    }

    // split on /(\r?\n)/ keeping the separators
    let mut lines: Vec<String> = Vec::new();
    let mut newline_sequences: Vec<String> = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0usize;
    while i < len {
        if units[i] == 10 {
            lines.push(String::from_utf16_lossy(&cur));
            cur.clear();
            newline_sequences.push("\n".to_string());
            i += 1;
        } else if units[i] == 13 && units.get(i + 1) == Some(&10) {
            lines.push(String::from_utf16_lossy(&cur));
            cur.clear();
            newline_sequences.push("\r\n".to_string());
            i += 2;
        } else {
            cur.push(units[i]);
            i += 1;
        }
    }
    lines.push(String::from_utf16_lossy(&cur));

    let line_len = |s: &str| s.encode_utf16().count();

    let mut count = 0usize;
    let mut res: Vec<String> = Vec::new();
    for i in 0..lines.len() {
        count += line_len(&lines[i])
            + newline_sequences.get(i).map(|s| s.len()).unwrap_or(0);
        if count >= start {
            let mut j = i as i64 - RANGE;
            while j <= i as i64 + RANGE || end > count {
                if j < 0 || j as usize >= lines.len() {
                    j += 1;
                    continue;
                }
                let ju = j as usize;
                let line = ju + 1;
                let pad_line = 3usize.saturating_sub(line.to_string().len());
                res.push(format!(
                    "{line}{}|  {}",
                    " ".repeat(pad_line),
                    lines[ju]
                ));
                let l_len = line_len(&lines[ju]);
                let nl_len = newline_sequences.get(ju).map(|s| s.len()).unwrap_or(0);

                if ju == i {
                    let pad = start as i64 - (count as i64 - (l_len + nl_len) as i64);
                    let pad = pad.max(0) as usize;
                    let length = if end > count {
                        (l_len as i64 - pad as i64).max(1) as usize
                    } else {
                        (end as i64 - start as i64).max(1) as usize
                    };
                    res.push(format!("   |  {}{}", " ".repeat(pad), "^".repeat(length)));
                } else if ju > i {
                    if end > count {
                        let length = ((end - count).min(l_len)).max(1);
                        res.push(format!("   |  {}", "^".repeat(length)));
                    }
                    count += l_len + nl_len;
                }
                j += 1;
            }
            break;
        }
    }
    res.join("\n")
}
