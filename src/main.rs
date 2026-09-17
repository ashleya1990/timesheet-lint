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

struct Entry {
    date: String,
    start_minutes: u32,
    end_minutes: u32,
    project: String,
}

fn lint(contents: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

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
            Ok(entry) => check_entry(line_no, &entry, &mut findings),
            Err(message) => findings.push(Finding {
                line: line_no,
                severity: Severity::Error,
                message,
            }),
        }
    }

    findings
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

    if !is_valid_date(date) {
        return Err(format!("'{date}' is not a valid date (expected YYYY-MM-DD)"));
    }
    let start_minutes = parse_time(start).map_err(|e| format!("start time '{start}': {e}"))?;
    let end_minutes = parse_time(end).map_err(|e| format!("end time '{end}': {e}"))?;

    Ok(Entry {
        date: date.to_string(),
        start_minutes,
        end_minutes,
        project: project.to_string(),
    })
}

fn is_valid_date(date: &str) -> bool {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return false;
    }
    if !y.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Ok(month) = m.parse::<u32>() else {
        return false;
    };
    let Ok(day) = d.parse::<u32>() else {
        return false;
    };
    (1..=12).contains(&month) && (1..=31).contains(&day)
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
    if entry.end_minutes <= entry.start_minutes {
        findings.push(Finding {
            line: line_no,
            severity: Severity::Error,
            message: format!(
                "end time is not after start time on {} (shifts crossing midnight are not supported yet)",
                entry.date
            ),
        });
        return;
    }

    let duration = entry.end_minutes - entry.start_minutes;
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
                expected: &[(2, Severity::Error, "end time is not after start time")],
            },
            Case {
                name: "end before start is rejected, not read as crossing midnight",
                input: "date,start,end,project\n2026-09-18,17:00,09:00,acme\n",
                expected: &[(2, Severity::Error, "end time is not after start time")],
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
