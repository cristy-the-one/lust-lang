// `lust explain L####`: long-form help for one diagnostic code.
//
// The text lives in docs/errors/, one Markdown file per code, and is built into the
// binary with `include_str!`. Each file has a wrong example that must fail with that
// code and a fixed example that must pass; scripts/check-explain.sh checks both against
// lust and rustc. docs/errors/REGISTRY.md is the checked list of in-use and reserved
// codes (scripts/check-codes.sh); it is built in too, so `lust explain` can list the
// reserved ranges and answer for a reserved code before any page exists for it.
/// All error docs, in code order.
const DOCS: &[(&str, &str)] = &[
    ("L0001", include_str!("../docs/errors/L0001.md")),
    ("L0002", include_str!("../docs/errors/L0002.md")),
    ("L0101", include_str!("../docs/errors/L0101.md")),
    ("L0102", include_str!("../docs/errors/L0102.md")),
    ("L0103", include_str!("../docs/errors/L0103.md")),
    ("L0104", include_str!("../docs/errors/L0104.md")),
    ("L0105", include_str!("../docs/errors/L0105.md")),
    ("L0106", include_str!("../docs/errors/L0106.md")),
    ("L0107", include_str!("../docs/errors/L0107.md")),
    ("L0110", include_str!("../docs/errors/L0110.md")),
    ("L0111", include_str!("../docs/errors/L0111.md")),
    ("L0112", include_str!("../docs/errors/L0112.md")),
    ("L0113", include_str!("../docs/errors/L0113.md")),
    ("L0114", include_str!("../docs/errors/L0114.md")),
    ("L0115", include_str!("../docs/errors/L0115.md")),
    ("L0116", include_str!("../docs/errors/L0116.md")),
    ("L0117", include_str!("../docs/errors/L0117.md")),
    ("L0118", include_str!("../docs/errors/L0118.md")),
    ("L0119", include_str!("../docs/errors/L0119.md")),
    ("L0120", include_str!("../docs/errors/L0120.md")),
    ("L0121", include_str!("../docs/errors/L0121.md")),
    ("L0122", include_str!("../docs/errors/L0122.md")),
    ("L0130", include_str!("../docs/errors/L0130.md")),
    ("L0201", include_str!("../docs/errors/L0201.md")),
    ("L0202", include_str!("../docs/errors/L0202.md")),
    ("L0203", include_str!("../docs/errors/L0203.md")),
    ("L0301", include_str!("../docs/errors/L0301.md")),
    ("L0302", include_str!("../docs/errors/L0302.md")),
    ("L0303", include_str!("../docs/errors/L0303.md")),
    ("L0304", include_str!("../docs/errors/L0304.md")),
    ("L0305", include_str!("../docs/errors/L0305.md")),
    ("L0306", include_str!("../docs/errors/L0306.md")),
    ("L0307", include_str!("../docs/errors/L0307.md")),
    ("L0308", include_str!("../docs/errors/L0308.md")),
    ("L0309", include_str!("../docs/errors/L0309.md")),
    ("L0310", include_str!("../docs/errors/L0310.md")),
    ("L0311", include_str!("../docs/errors/L0311.md")),
    ("L0312", include_str!("../docs/errors/L0312.md")),
    ("L0401", include_str!("../docs/errors/L0401.md")),
    ("L0402", include_str!("../docs/errors/L0402.md")),
    ("L0403", include_str!("../docs/errors/L0403.md")),
    ("L0404", include_str!("../docs/errors/L0404.md")),
    ("L0405", include_str!("../docs/errors/L0405.md")),
    ("L0502", include_str!("../docs/errors/L0502.md")),
    ("L0503", include_str!("../docs/errors/L0503.md")),
    ("L0505", include_str!("../docs/errors/L0505.md")),
    ("L0506", include_str!("../docs/errors/L0506.md")),
    ("L0507", include_str!("../docs/errors/L0507.md")),
    ("L0508", include_str!("../docs/errors/L0508.md")),
    ("L0509", include_str!("../docs/errors/L0509.md")),
    ("L0510", include_str!("../docs/errors/L0510.md")),
    ("L0511", include_str!("../docs/errors/L0511.md")),
    ("L0520", include_str!("../docs/errors/L0520.md")),
    ("L0530", include_str!("../docs/errors/L0530.md")),
    ("L0531", include_str!("../docs/errors/L0531.md")),
    ("L0532", include_str!("../docs/errors/L0532.md")),
    ("L0522", include_str!("../docs/errors/L0522.md")),
    ("L0523", include_str!("../docs/errors/L0523.md")),
    ("L0524", include_str!("../docs/errors/L0524.md")),
    ("L0540", include_str!("../docs/errors/L0540.md")),
    ("L0541", include_str!("../docs/errors/L0541.md")),
    ("L0542", include_str!("../docs/errors/L0542.md")),
    ("L0543", include_str!("../docs/errors/L0543.md")),
    ("L0544", include_str!("../docs/errors/L0544.md")),
    ("L0545", include_str!("../docs/errors/L0545.md")),
    ("L0546", include_str!("../docs/errors/L0546.md")),
    ("L0601", include_str!("../docs/errors/L0601.md")),
    ("L0602", include_str!("../docs/errors/L0602.md")),
    ("L0603", include_str!("../docs/errors/L0603.md")),
    ("L0610", include_str!("../docs/errors/L0610.md")),
    ("L0611", include_str!("../docs/errors/L0611.md")),
    ("L0612", include_str!("../docs/errors/L0612.md")),
    ("L0613", include_str!("../docs/errors/L0613.md")),
    ("L0614", include_str!("../docs/errors/L0614.md")),
    ("L0615", include_str!("../docs/errors/L0615.md")),
    ("L0616", include_str!("../docs/errors/L0616.md")),
    ("L0617", include_str!("../docs/errors/L0617.md")),
    ("L0621", include_str!("../docs/errors/L0621.md")),
    ("L0622", include_str!("../docs/errors/L0622.md")),
    ("L0632", include_str!("../docs/errors/L0632.md")),
    ("L0630", include_str!("../docs/errors/L0630.md")),
    ("L0631", include_str!("../docs/errors/L0631.md")),
    ("L0633", include_str!("../docs/errors/L0633.md")),
    ("L0634", include_str!("../docs/errors/L0634.md")),
    ("L0640", include_str!("../docs/errors/L0640.md")),
    ("L0641", include_str!("../docs/errors/L0641.md")),
    ("L0642", include_str!("../docs/errors/L0642.md")),
    ("L0643", include_str!("../docs/errors/L0643.md")),
    ("L0650", include_str!("../docs/errors/L0650.md")),
    ("L0651", include_str!("../docs/errors/L0651.md")),
    ("L0652", include_str!("../docs/errors/L0652.md")),
    ("L0701", include_str!("../docs/errors/L0701.md")),
    ("L0702", include_str!("../docs/errors/L0702.md")),
    ("L0703", include_str!("../docs/errors/L0703.md")),
    ("L0704", include_str!("../docs/errors/L0704.md")),
    ("L0705", include_str!("../docs/errors/L0705.md")),
    ("L0720", include_str!("../docs/errors/L0720.md")),
    ("L0721", include_str!("../docs/errors/L0721.md")),
    ("L0730", include_str!("../docs/errors/L0730.md")),
    ("L0731", include_str!("../docs/errors/L0731.md")),
    ("L0733", include_str!("../docs/errors/L0733.md")),
    ("L0734", include_str!("../docs/errors/L0734.md")),
    ("L0735", include_str!("../docs/errors/L0735.md")),
    ("L0801", include_str!("../docs/errors/L0801.md")),
    ("L0802", include_str!("../docs/errors/L0802.md")),
    ("L0803", include_str!("../docs/errors/L0803.md")),
    ("L0805", include_str!("../docs/errors/L0805.md")),
    ("L0806", include_str!("../docs/errors/L0806.md")),
    ("L0807", include_str!("../docs/errors/L0807.md")),
    ("L0809", include_str!("../docs/errors/L0809.md")),
    ("L0810", include_str!("../docs/errors/L0810.md")),
    ("L0820", include_str!("../docs/errors/L0820.md")),
    ("L9001", include_str!("../docs/errors/L9001.md")),
];

/// The registry table from docs/errors/REGISTRY.md, built into the binary.
const REGISTRY: &str = include_str!("../docs/errors/REGISTRY.md");

/// One registry row: a code or an inclusive range, with its area and owner.
struct Range {
    lo: u32,
    hi: u32,
    area: &'static str,
    owner: &'static str,
    reserved: bool,
    notes: &'static str,
}

/// The number in a code like `L####`, or None if the text is not a code.
fn code_num(s: &str) -> Option<u32> {
    let digits = s.strip_prefix('L')?;
    if digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()) { digits.parse().ok() } else { None }
}

/// Parses the registry table rows. Rows that do not parse are skipped here;
/// scripts/check-codes.sh is the gate that rejects a malformed registry.
fn ranges() -> Vec<Range> {
    let mut out = Vec::new();
    for line in REGISTRY.lines() {
        let f: Vec<&str> = line.split('|').map(str::trim).collect();
        // f is ["", range, area, owner, status, notes, ""] for a table row.
        if f.len() < 6 {
            continue;
        }
        let (lo, hi) = match f[1].split_once('-') {
            Some((a, b)) => match (code_num(a), code_num(b)) {
                (Some(lo), Some(hi)) => (lo, hi),
                _ => continue,
            },
            None => match code_num(f[1]) {
                Some(n) => (n, n),
                None => continue,
            },
        };
        let reserved = match f[4] {
            "reserved" => true,
            "in use" => false,
            _ => continue,
        };
        out.push(Range { lo, hi, area: f[2], owner: f[3], reserved, notes: f[5] });
    }
    out
}

/// Prints a range as one code or as `L####-L####`.
fn range_text(r: &Range) -> String {
    if r.lo == r.hi { format!("L{:04}", r.lo) } else { format!("L{:04}-L{:04}", r.lo, r.hi) }
}

/// The title of a doc: its first line, without the leading `# `.
fn title(doc: &str) -> &str {
    doc.lines().next().unwrap_or("").trim_start_matches("# ")
}

/// Entry point for `lust explain`. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    match args {
        [] => {
            println!("usage: lust explain <L####>");
            println!("codes:");
            for (_, doc) in DOCS {
                println!("  {}", title(doc));
            }
            let reserved: Vec<Range> = ranges().into_iter().filter(|r| r.reserved).collect();
            if !reserved.is_empty() {
                println!("reserved (not yet emitted):");
                for r in &reserved {
                    println!("  {}: {} ({})", range_text(r), r.area, r.owner);
                }
            }
            0
        }
        [arg] => {
            let want = arg.to_ascii_uppercase();
            if let Some((_, doc)) = DOCS.iter().find(|(code, _)| *code == want) {
                print!("{doc}");
                return 0;
            }
            if let Some(n) = code_num(&want) {
                if let Some(r) = ranges().iter().find(|r| r.reserved && r.lo <= n && n <= r.hi) {
                    println!("{want}: reserved, not yet emitted");
                    println!("area: {}", r.area);
                    println!("owner: {}", r.owner);
                    if !r.notes.is_empty() {
                        println!("planned: {}", r.notes);
                    }
                    return 0;
                }
            }
            eprintln!("error: unknown error code `{arg}`");
            eprintln!("run `lust explain` to list all codes");
            2
        }
        _ => {
            eprintln!("usage: lust explain <L####>");
            2
        }
    }
}

/// Every documented code. Used by `lust capabilities`.
pub fn codes() -> Vec<(&'static str, String)> {
    DOCS.iter().map(|(code, doc)| (*code, title(doc).split(": ").nth(1).unwrap_or("").to_string())).collect()
}
