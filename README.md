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
examples/sample.csv:6: error: start and end time are both 09:00 on 2026-09-17, a zero-length shift
```

Findings are printed as `path:line: severity: message`. The process exits
with status 1 if any finding is an `error`, 0 if there are only `warning`s
or none at all, and 2 if the file couldn't be read.

Current checks:

- malformed rows (wrong field count, bad date, bad time)
- zero-length shifts (start equal to end)
- overlapping shifts, across rows, including an overnight shift running into
  the next day's rows
- shifts longer than 16 hours (warning)
- empty project field (warning)

An end time earlier than the start time is read as a shift that crosses
midnight and ends on the following day, so `22:00,05:00` is a 7 hour shift.
Equal start and end times are never read as a 24 hour shift.

Overlap checking leaves out zero-length rows; those are reported on their
own.

## Building

Standard `cargo build` / `cargo test`, no external crates.

## Status

Early. The rule set above is deliberately small. Next up is a JSON output
mode for CI, then a configurable shift length limit.
