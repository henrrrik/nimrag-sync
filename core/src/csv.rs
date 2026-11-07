//! The CSV of the device's tables: rows separated by `\r\n` (or a bare `\n`
//! or `\r`), fields by commas, a field in double quotes when it holds a comma,
//! a quote or a line break, a quote inside one doubled. No header row.

/// The rows of a table as `READ_TABLE` sends them. Empty lines are skipped.
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    // Whether the current row has anything yet, so empty lines are dropped.
    let mut started = false;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                started = true;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                started = true;
            }
            '\r' | '\n' => {
                if started {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                started = false;
            }
            _ => {
                field.push(c);
                started = true;
            }
        }
    }
    if started {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// A field as inReach Sync's `CsvTools.CsvEncodeString` writes it: quoted
/// only when it holds a comma, a quote or a line break; blank becomes empty.
pub fn encode(value: &str) -> String {
    if value.trim().is_empty() {
        return String::new();
    }
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_fields_and_quotes() {
        let text = "1,0,\"\",\"Ada\",,\"a, \"\"b\"\"\"\r\n\r\n9,300434000000000,\r\n26,\"x@inreach.garmin.com\",\"\"";
        assert_eq!(
            parse(text),
            vec![
                vec!["1", "0", "", "Ada", "", "a, \"b\""],
                vec!["9", "300434000000000", ""],
                vec!["26", "x@inreach.garmin.com", ""],
            ]
        );
    }

    #[test]
    fn a_quoted_field_may_hold_a_line_break() {
        assert_eq!(parse("1,\"two\r\nlines\"\n2,x"), vec![vec!["1", "two\r\nlines"], vec!["2", "x"]]);
    }

    #[test]
    fn non_ascii_survives() {
        assert_eq!(parse("58,\"Stuga vid sjön\",\"\""), vec![vec!["58", "Stuga vid sjön", ""]]);
    }

    #[test]
    fn encode_quotes_only_when_needed() {
        assert_eq!(encode("Ada"), "Ada");
        assert_eq!(encode("  "), "");
        assert_eq!(encode("a,b"), "\"a,b\"");
        assert_eq!(encode("say \"hi\""), "\"say \"\"hi\"\"\"");
        // What is parsed back is what was encoded.
        for v in ["Ada", "a,b", "say \"hi\"", "två\r\nrader"] {
            assert_eq!(parse(&format!("1,{}", encode(v)))[0][1], v);
        }
    }
}
