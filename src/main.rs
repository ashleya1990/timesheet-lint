use std::env;
use std::fmt;
use std::fs;
use std::process::ExitCode;

const MAX_SHIFT_MINUTES: u32 = 16 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Finding {
    line: usize,
    severity: Severity,
    message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.line, self.severity, self.message)
    }
}

const MINUTES_PER_DAY: u32 = 24 * 60;

struct Entry {
    date: String,
    // Days since 1970-01-01, so shifts on different dates can be placed on one timeline.
    day: i64,
    start_minutes: u32,
    end_minutes: u32,
    project: String,
}

impl Entry {
    // An end time earlier than the start time means the shift runs past midnight into
    // the next day. Equal times are a zero-length shift, not a 24 hour one.
    fn duration(&self) -> Option<u32> {
        if self.end_minutes == self.start_minutes {
            None
        } else if self.end_minutes > self.start_minutes {
            Some(self.end_minutes - self.start_minutes)
        } else {
            Some(MINUTES_PER_DAY - self.start_minutes + self.end_minutes)
        }
    }

    // Start and end as minutes since the epoch day, so an overnight shift can be
    // compared against the next day's rows.
    fn absolute_range(&self) -> Option<(i64, i64)> {
        let duration = self.duration()? as i64;
        let start = self.day * MINUTES_PER_DAY as i64 + self.start_minutes as i64;
        Some((start, start + duration))
    }
}

fn lint(contents: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut entries: Vec<(usize, Entry)> = Vec::new();

    for (idx, raw_line) in contents.lines().enumerate() {
        let line_no = idx + 1;
        // Row 1 is always the column header, whatever it says, so it is never checked.
        if line_no == 1 {
            continue;
        }
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_entry(line) {
            Ok(entry) => {
                check_entry(line_no, &entry, &mut findings);
                entries.push((line_no, entry));
            }
            Err(message) => findings.push(Finding {
                line: line_no,
                severity: Severity::Error,
                message,
            }),
        }
    }

    check_overlaps(&entries, &mut findings);
    // Per-row checks land in line order already, but overlap findings are appended
    // after the fact and can point at an earlier line than whatever was checked last.
    findings.sort_by_key(|f| f.line);
    findings
}

// Compares every pair of entries on a shared timeline and flags ranges that intersect.
// Comparing across dates is what catches an overnight shift running into the next
// day's first shift. Zero-length entries are skipped since check_entry already
// reported them and they have no meaningful overlap to report.
fn check_overlaps(entries: &[(usize, Entry)], findings: &mut Vec<Finding>) {
    for i in 0..entries.len() {
        let (line_a, a) = &entries[i];
        let Some((a_start, a_end)) = a.absolute_range() else {
            continue;
        };
        for (line_b, b) in &entries[i + 1..] {
            let Some((b_start, b_end)) = b.absolute_range() else {
                continue;
            };
            if a_start < b_end && b_start < a_end {
                findings.push(Finding {
                    line: *line_b,
                    severity: Severity::Error,
                    message: format!(
                        "shift {} {}-{} overlaps with the shift on line {} ({} {}-{})",
                        b.date,
                        format_time(b.start_minutes),
                        format_time(b.end_minutes),
                        line_a,
                        a.date,
                        format_time(a.start_minutes),
                        format_time(a.end_minutes),
                    ),
                });
            }
        }
    }
}

fn format_time(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn parse_entry(line: &str) -> Result<Entry, String> {
    let fields: Vec<&str> = line.split(',').map(|f| f.trim()).collect();
    if fields.len() != 4 {
        return Err(format!(
            "expected 4 comma-separated fields (date,start,end,project), found {}",
            fields.len()
        ));
    }
    let (date, start, end, project) = (fields[0], fields[1], fields[2], fields[3]);

    let Some((year, month, day)) = parse_date(date) else {
        return Err(format!("'{date}' is not a valid date (expected YYYY-MM-DD)"));
    };
    let start_minutes = parse_time(start).map_err(|e| format!("start time '{start}': {e}"))?;
    let end_minutes = parse_time(end).map_err(|e| format!("end time '{end}': {e}"))?;

    Ok(Entry {
        date: date.to_string(),
        day: days_from_civil(year, month, day),
        start_minutes,
        end_minutes,
        project: project.to_string(),
    })
}

fn parse_date(date: &str) -> Option<(i64, u32, u32)> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    if !y.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year = y.parse::<i64>().ok()?;
    let month = m.parse::<u32>().ok()?;
    let day = d.parse::<u32>().ok()?;
    if (1..=12).contains(&month) && (1..=31).contains(&day) {
        Some((year, month, day))
    } else {
        None
    }
}

// Days since 1970-01-01 in the proleptic Gregorian calendar (Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 } as i64;
    let doy = (153 * shifted_month + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn parse_time(value: &str) -> Result<u32, String> {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 2 {
        return Err("expected HH:MM".to_string());
    }
    let hour: u32 = parts[0]
        .parse()
        .map_err(|_| "hour is not a number".to_string())?;
    let minute: u32 = parts[1]
        .parse()
        .map_err(|_| "minute is not a number".to_string())?;
    if hour > 23 {
        return Err(format!("hour {hour} is out of range (0-23)"));
    }
    if minute > 59 {
        return Err(format!("minute {minute} is out of range (0-59)"));
    }
    Ok(hour * 60 + minute)
}

fn check_entry(line_no: usize, entry: &Entry, findings: &mut Vec<Finding>) {
    let Some(duration) = entry.duration() else {
        findings.push(Finding {
            line: line_no,
            severity: Severity::Error,
            message: format!(
                "start and end time are both {} on {}, a zero-length shift",
                format_time(entry.start_minutes),
                entry.date
            ),
        });
        return;
    };

    if duration > MAX_SHIFT_MINUTES {
        findings.push(Finding {
            line: line_no,
            severity: Severity::Warning,
            message: format!(
                "shift is {:.1} hours long, longer than the 16 hour sanity limit",
                duration as f64 / 60.0
            ),
        });
    }

    if entry.project.is_empty() {
        findings.push(Finding {
            line: line_no,
            severity: Severity::Warning,
            message: "project field is empty".to_string(),
        });
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: timesheet-lint <file.csv>");
            return ExitCode::from(2);
        }
    };

    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("could not read {path}: {e}");
            return ExitCode::from(2);
        }
    };

    let findings = lint(&contents);
    let has_errors = findings.iter().any(|f| f.severity == Severity::Error);

    if findings.is_empty() {
        println!("{path}: no findings");
    } else {
        for finding in &findings {
            println!("{path}:{finding}");
        }
    }

    if has_errors {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        name: &'static str,
        input: &'static str,
        // (line, severity, substring expected in the finding's message)
        expected: &'static [(usize, Severity, &'static str)],
    }

    #[test]
    fn table_driven_cases() {
        let cases = vec![
            Case {
                name: "clean single entry",
                input: "date,start,end,project\n2026-09-18,09:00,17:30,acme\n",
                expected: &[],
            },
            Case {
                name: "header row is never linted, even if malformed",
                input: "not,a,real,header,at,all\n2026-09-18,09:00,17:00,acme\n",
                expected: &[],
            },
            Case {
                name: "blank and whitespace-only lines are skipped",
                input: "date,start,end,project\n\n2026-09-18,09:00,17:00,acme\n   \n",
                expected: &[],
            },
            Case {
                name: "too few fields",
                input: "date,start,end,project\n2026-09-18,09:00,17:00\n",
                expected: &[(2, Severity::Error, "4 comma-separated fields")],
            },
            Case {
                name: "too many fields",
                input: "date,start,end,project\n2026-09-18,09:00,17:00,acme,extra\n",
                expected: &[(2, Severity::Error, "4 comma-separated fields")],
            },
            Case {
                name: "bad date format",
                input: "date,start,end,project\n09-18-2026,09:00,17:00,acme\n",
                expected: &[(2, Severity::Error, "not a valid date")],
            },
            Case {
                name: "time missing a colon",
                input: "date,start,end,project\n2026-09-18,0900,17:00,acme\n",
                expected: &[(2, Severity::Error, "expected HH:MM")],
            },
            Case {
                name: "hour out of range",
                input: "date,start,end,project\n2026-09-18,25:00,17:00,acme\n",
                expected: &[(2, Severity::Error, "hour 25 is out of range")],
            },
            Case {
                name: "minute out of range",
                input: "date,start,end,project\n2026-09-18,09:65,17:00,acme\n",
                expected: &[(2, Severity::Error, "minute 65 is out of range")],
            },
            Case {
                name: "end equal to start is a zero-length shift",
                input: "date,start,end,project\n2026-09-18,09:00,09:00,acme\n",
                expected: &[(2, Severity::Error, "zero-length shift")],
            },
            Case {
                name: "end before start is read as crossing midnight",
                input: "date,start,end,project\n2026-09-18,22:00,05:00,oncall\n",
                expected: &[],
            },
            Case {
                name: "overnight shift of exactly 16 hours is fine",
                input: "date,start,end,project\n2026-09-18,17:00,09:00,acme\n",
                expected: &[],
            },
            Case {
                name: "overnight shift one minute past 16 hours warns",
                input: "date,start,end,project\n2026-09-18,17:00,09:01,acme\n",
                expected: &[(2, Severity::Warning, "longer than the 16 hour")],
            },
            Case {
                name: "overnight shift overlaps the next day's early shift",
                input: "date,start,end,project\n2026-09-18,22:00,05:00,oncall\n2026-09-19,04:00,08:00,acme\n",
                expected: &[(3, Severity::Error, "overlaps with the shift on line 2")],
            },
            Case {
                name: "overnight shift ending when the next day's shift starts does not overlap",
                input: "date,start,end,project\n2026-09-18,22:00,05:00,oncall\n2026-09-19,05:00,08:00,acme\n",
                expected: &[],
            },
            Case {
                name: "overnight shift across a month and year boundary",
                input: "date,start,end,project\n2026-12-31,22:00,03:00,oncall\n2027-01-01,02:00,06:00,acme\n",
                expected: &[(3, Severity::Error, "overlaps with the shift on line 2")],
            },
            Case {
                name: "overnight shift does not overlap a shift the following evening",
                input: "date,start,end,project\n2026-09-18,22:00,05:00,oncall\n2026-09-19,09:00,17:00,acme\n",
                expected: &[],
            },
            Case {
                name: "shift exactly at the 16 hour limit is fine",
                input: "date,start,end,project\n2026-09-18,05:00,21:00,acme\n",
                expected: &[],
            },
            Case {
                name: "shift one minute past the 16 hour limit warns",
                input: "date,start,end,project\n2026-09-18,05:00,21:01,acme\n",
                expected: &[(2, Severity::Warning, "longer than the 16 hour")],
            },
            Case {
                name: "empty project field",
                input: "date,start,end,project\n2026-09-18,09:00,17:00,\n",
                expected: &[(2, Severity::Warning, "project field is empty")],
            },
            Case {
                name: "second data row keeps the correct line number",
                input: "date,start,end,project\n2026-09-18,09:00,17:00,acme\n2026-09-19,25:00,17:00,acme\n",
                expected: &[(3, Severity::Error, "hour 25 is out of range")],
            },
            Case {
                name: "overlapping shifts on the same date are flagged",
                input: "date,start,end,project\n2026-09-18,09:00,17:00,acme\n2026-09-18,16:00,18:00,other\n",
                expected: &[(3, Severity::Error, "overlaps with the shift on line 2")],
            },
            Case {
                name: "back-to-back shifts that only touch at the boundary do not overlap",
                input: "date,start,end,project\n2026-09-18,09:00,17:00,acme\n2026-09-18,17:00,18:00,other\n",
                expected: &[],
            },
            Case {
                name: "same times on different dates do not overlap",
                input: "date,start,end,project\n2026-09-18,09:00,17:00,acme\n2026-09-19,09:00,17:00,acme\n",
                expected: &[],
            },
            Case {
                name: "a zero-length shift is not treated as overlapping the next one",
                input: "date,start,end,project\n2026-09-18,09:00,09:00,acme\n2026-09-18,09:00,17:00,acme\n",
                expected: &[(2, Severity::Error, "zero-length shift")],
            },
            Case {
                name: "an overlap finding sorts before a later parse error on a higher line",
                input: "date,start,end,project\n2026-09-18,09:00,17:00,acme\n2026-09-18,16:00,18:00,other\n2026-09-19,25:00,17:00,acme\n",
                expected: &[
                    (3, Severity::Error, "overlaps with the shift on line 2"),
                    (4, Severity::Error, "hour 25 is out of range"),
                ],
            },
        ];

        for case in cases {
            let findings = lint(case.input);
            assert_eq!(
                findings.len(),
                case.expected.len(),
                "case '{}': expected {} findings, got {:?}",
                case.name,
                case.expected.len(),
                findings
            );
            for (finding, (line, severity, substring)) in findings.iter().zip(case.expected.iter()) {
                assert_eq!(finding.line, *line, "case '{}': line mismatch", case.name);
                assert_eq!(
                    finding.severity, *severity,
                    "case '{}': severity mismatch",
                    case.name
                );
                assert!(
                    finding.message.contains(substring),
                    "case '{}': expected message to contain '{}', got '{}'",
                    case.name,
                    substring,
                    finding.message
                );
            }
        }
    }
}
