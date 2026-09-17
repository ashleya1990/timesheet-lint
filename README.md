# timesheet-lint

A lot of timesheet data lives in a CSV that someone exports from a time
tracker, edits by hand in a spreadsheet, and pastes back in. The format
tolerates almost anything: a shift where the clock-out got typed as `9:00`
instead of `19:00`, a project column left blank, an hour field that reads
`25:00` because someone fat-fingered the export. None of that fails loudly,
it just quietly corrupts payroll or billing numbers downstream.

`timesheet-lint` reads a timesheet CSV and reports problems with the line
number they came from, so you can fix the row instead of grepping for it.

## Input format

One shift per line, four comma-separated fields, with a header row on line 1:

```
date,start,end,project
2026-09-14,09:00,17:30,acme-website
```

- `date` is `YYYY-MM-DD`.
- `start` and `end` are 24-hour `HH:MM`.
- `project` is a free-text label; it can be any non-empty string.

## Usage

```
$ timesheet-lint examples/sample.csv
examples/sample.csv:5: error: end time is not after start time on 2026-09-17 (shifts crossing midnight are not supported yet)
examples/sample.csv:4: error: end time is not after start time on 2026-09-16 (shifts crossing midnight are not supported yet)
```

Findings are printed as `path:line: severity: message`. The process exits
with status 1 if any finding is an `error`, 0 if there are only `warning`s
or none at all, and 2 if the file couldn't be read.

Current checks:

- malformed rows (wrong field count, bad date, bad time)
- end time not after start time
- shifts longer than 16 hours (warning)
- empty project field (warning)

Shifts that cross midnight (end time earlier than start time on the same
line) are currently flagged as an error rather than understood as spanning
two days — see the roadmap below.

## Building

Standard `cargo build` / `cargo test`, no external crates.

## Status

Early skeleton. The rule set above is deliberately small; see the roadmap
in the issue tracker for what's planned next, starting with detecting
overlapping shifts across multiple rows for the same date.
