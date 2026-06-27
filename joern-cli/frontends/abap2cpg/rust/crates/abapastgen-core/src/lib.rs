use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Number of statements that fell through to the `Unknown` classification.
///
/// The CLI prints a single stderr summary line at the end of a run so the
/// classifier's blind spots stay visible without polluting stdout/JSON.
static UNCLASSIFIED_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Total statements classified as `Unknown` since process start.
pub fn unclassified_count() -> usize {
    UNCLASSIFIED_COUNT.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Token {
    pub str: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Statement {
    #[serde(rename = "type")]
    pub kind: String,
    pub tokens: Vec<Token>,
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Program {
    pub file: String,
    #[serde(rename = "objectType")]
    pub object_type: String,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawStatement {
    text: String,
    start: Position,
    end: Position,
}

pub fn generate_file(path: &Path, display_file: &str) -> Result<Program> {
    let source = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(generate_source(&source, display_file, object_type(path)))
}

pub fn generate_source(source: &str, display_file: &str, object_type: &str) -> Program {
    let statements = split_statements(source)
        .into_iter()
        .map(|raw| {
            let tokens = if raw.text.trim_start().starts_with('*') {
                vec![raw.text.clone()]
            } else {
                tokenize_statement(&raw.text)
            };
            let kind = classify_statement(&tokens);
            Statement {
                kind,
                tokens: tokens.into_iter().map(|str| Token { str }).collect(),
                start: raw.start,
                end: raw.end,
            }
        })
        .filter(|stmt| !stmt.tokens.is_empty())
        .collect();

    Program {
        file: display_file.to_string(),
        object_type: object_type.to_string(),
        statements,
    }
}

fn object_type(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains(".clas.") {
        "CLAS"
    } else if name.contains(".fugr.") {
        "FUGR"
    } else {
        "PROG"
    }
}

fn split_statements(source: &str) -> Vec<RawStatement> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut start: Option<Position> = None;
    let mut in_single = false;
    let mut in_backtick = false;
    let mut in_template = false;

    for (line_idx, line) in source.lines().enumerate() {
        let row = line_idx + 1;
        if let Some(comment_start) = line.find(|ch: char| !ch.is_whitespace()) {
            if line[comment_start..].starts_with('*') {
                let text = line[comment_start..].trim_end().to_string();
                if !text.is_empty() {
                    result.push(RawStatement {
                        text,
                        start: Position {
                            row,
                            col: line[..comment_start].chars().count() + 1,
                        },
                        end: Position {
                            row,
                            col: line.trim_end().chars().count() + 1,
                        },
                    });
                }
                continue;
            }
        } else {
            continue;
        }

        let line = strip_inline_comment(line);
        let mut chars = line.char_indices().peekable();
        while let Some((byte_idx, ch)) = chars.next() {
            let col = line[..byte_idx].chars().count() + 1;
            if start.is_none() && ch.is_whitespace() {
                continue;
            }
            if start.is_none() {
                start = Some(Position { row, col });
            }

            current.push(ch);

            match ch {
                '\'' if !in_backtick && !in_template => {
                    if in_single && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                        if let Some((_, next)) = chars.next() {
                            current.push(next);
                        }
                    } else {
                        in_single = !in_single;
                    }
                }
                '`' if !in_single && !in_template => in_backtick = !in_backtick,
                '|' if !in_single && !in_backtick => in_template = !in_template,
                '.' if !in_single && !in_backtick && !in_template => {
                    if let Some(start) = start.take() {
                        let text = current.trim().to_string();
                        if !text.is_empty() {
                            result.push(RawStatement {
                                text,
                                start,
                                end: Position { row, col: col + 1 },
                            });
                        }
                    }
                    current.clear();
                }
                _ => {}
            }
        }

        if start.is_some() {
            current.push('\n');
        }
    }

    if let Some(start) = start {
        let text = current.trim().to_string();
        if !text.is_empty() {
            let end_col = text.lines().last().map(|x| x.chars().count()).unwrap_or(1);
            result.push(RawStatement {
                text,
                start,
                end: Position {
                    row: source.lines().count().max(1),
                    col: end_col.max(1) + 1,
                },
            });
        }
    }

    result
}

fn strip_inline_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_backtick = false;
    let mut in_template = false;
    let mut chars = line.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\'' if !in_backtick && !in_template => {
                if in_single && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    let _ = chars.next();
                } else {
                    in_single = !in_single;
                }
            }
            '`' if !in_single && !in_template => in_backtick = !in_backtick,
            '|' if !in_single && !in_backtick => in_template = !in_template,
            '"' if !in_single && !in_backtick && !in_template => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn tokenize_statement(statement: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = statement.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }

        match ch {
            '\'' | '`' | '|' => {
                tokens.push(read_quoted(statement, idx, ch));
                skip_until(
                    &mut chars,
                    idx + tokens.last().map(|x| x.len()).unwrap_or_default(),
                );
            }
            '=' if chars.peek().is_some_and(|(_, next)| *next == '>') => {
                let _ = chars.next();
                tokens.push("=>".to_string());
            }
            '-' if chars.peek().is_some_and(|(_, next)| *next == '>') => {
                let _ = chars.next();
                tokens.push("->".to_string());
            }
            '&' if chars.peek().is_some_and(|(_, next)| *next == '&') => {
                let _ = chars.next();
                tokens.push("&&".to_string());
            }
            '(' | ')' | ',' | ':' | '.' | '=' | '+' | '*' | '/' => tokens.push(ch.to_string()),
            '-' => tokens.push("-".to_string()),
            _ if is_word_char(ch) => {
                let word = read_word(statement, idx);
                skip_until(&mut chars, idx + word.len());
                tokens.push(word);
            }
            _ => tokens.push(ch.to_string()),
        }
    }
    tokens
}

fn read_quoted(statement: &str, start: usize, quote: char) -> String {
    let mut escaped_single = false;
    let mut end = statement.len();
    let mut iter = statement[start + quote.len_utf8()..]
        .char_indices()
        .peekable();
    while let Some((offset, ch)) = iter.next() {
        if quote == '\'' && ch == '\'' && iter.peek().is_some_and(|(_, next)| *next == '\'') {
            escaped_single = true;
            let _ = iter.next();
            continue;
        }
        if ch == quote && !(quote == '\'' && escaped_single) {
            end = start + quote.len_utf8() + offset + ch.len_utf8();
            break;
        }
        escaped_single = false;
    }
    statement[start..end].to_string()
}

fn read_word(statement: &str, start: usize) -> String {
    let end = statement[start..]
        .char_indices()
        .find_map(|(idx, ch)| (!is_word_char(ch)).then_some(start + idx))
        .unwrap_or(statement.len());
    statement[start..end].to_string()
}

fn skip_until<I>(chars: &mut std::iter::Peekable<I>, target: usize)
where
    I: Iterator<Item = (usize, char)>,
{
    while chars.peek().is_some_and(|(idx, _)| *idx < target) {
        let _ = chars.next();
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '#' | '~')
}

fn classify_statement(tokens: &[String]) -> String {
    let upper: Vec<String> = tokens.iter().map(|x| x.to_ascii_uppercase()).collect();
    let has = |needle: &str| upper.iter().any(|x| x == needle);
    let starts = |parts: &[&str]| {
        parts
            .iter()
            .enumerate()
            .all(|(idx, part)| upper.get(idx).is_some_and(|x| x == part))
    };

    if tokens
        .first()
        .is_some_and(|x| x.trim_start().starts_with('*'))
    {
        "Comment"
    } else if starts(&["CLASS"]) && upper.iter().any(|x| x == "DEFINITION") {
        "ClassDefinition"
    } else if starts(&["CLASS"]) && upper.iter().any(|x| x == "IMPLEMENTATION") {
        "ClassImplementation"
    } else if starts(&["ENDCLASS"]) {
        "EndClass"
    } else if starts(&["CLASS-METHODS"]) || starts(&["CLASS", "-", "METHODS"]) {
        "MethodDef"
    } else if starts(&["METHODS"]) {
        "MethodDef"
    } else if starts(&["METHOD"]) {
        "MethodImplementation"
    } else if starts(&["ENDMETHOD"]) {
        "EndMethod"
    } else if starts(&["FORM"]) {
        "Form"
    } else if starts(&["ENDFORM"]) {
        "EndForm"
    } else if starts(&["FUNCTION"]) {
        "Function"
    } else if starts(&["ENDFUNCTION"]) {
        "EndFunction"
    } else if starts(&["OPEN", "DATASET"]) {
        "OpenDataset"
    } else if starts(&["READ", "DATASET"]) {
        "ReadDataset"
    } else if starts(&["DELETE", "DATASET"]) {
        "DeleteDataset"
    } else if starts(&["DELETE", "DYNPRO"]) {
        "Unknown"
    } else if starts(&["TRANSFER"]) {
        "Transfer"
    } else if starts(&["AUTHORITY-CHECK"]) || starts(&["AUTHORITY", "-", "CHECK"]) {
        "AuthorityCheck"
    } else if starts(&["GENERATE", "SUBROUTINE"]) {
        "GenerateSubroutine"
    } else if starts(&["CALL", "TRANSFORMATION"]) {
        "Unknown"
    } else if starts(&["EDITOR-CALL"]) || starts(&["EDITOR", "-", "CALL"]) {
        "EditorCall"
    } else if starts(&["IF"]) {
        "If"
    } else if starts(&["ELSEIF"]) {
        "ElseIf"
    } else if starts(&["ELSE"]) {
        "Else"
    } else if starts(&["ENDIF"]) {
        "EndIf"
    } else if starts(&["CASE"]) {
        "Case"
    } else if starts(&["WHEN", "OTHERS"]) {
        "WhenOthers"
    } else if starts(&["WHEN"]) {
        "When"
    } else if starts(&["ENDCASE"]) {
        "EndCase"
    } else if starts(&["WHILE"]) {
        "While"
    } else if starts(&["ENDWHILE"]) {
        "EndWhile"
    } else if starts(&["DO"]) {
        "Do"
    } else if starts(&["ENDDO"]) {
        "EndDo"
    } else if starts(&["LOOP"]) {
        "Loop"
    } else if starts(&["ENDLOOP"]) {
        "EndLoop"
    } else if starts(&["TRY"]) {
        "Try"
    } else if starts(&["CATCH"]) {
        "Catch"
    } else if starts(&["CLEANUP"]) {
        "Cleanup"
    } else if starts(&["ENDTRY"]) {
        "EndTry"
    } else if starts(&["CHECK"]) {
        "Check"
    } else if starts(&["EXIT"]) {
        "Exit"
    } else if starts(&["CONTINUE"]) {
        "Continue"
    } else if starts(&["RETURN"]) {
        "Return"
    } else if starts(&["RAISE"]) {
        "Raise"
    } else if starts(&["CALL", "FUNCTION"]) {
        "CallFunction"
    } else if starts(&["CALL"]) {
        "Call"
    } else if starts(&["DATA"]) && has("=") {
        "Move"
    } else if starts(&["DATA"]) {
        "Data"
    } else if starts(&["MOVE"]) {
        "Move"
    } else if starts(&["ASSIGN"]) {
        "Assign"
    } else if is_method_call_statement(tokens) {
        "Call"
    } else if has("=") {
        "Move"
    } else {
        UNCLASSIFIED_COUNT.fetch_add(1, Ordering::Relaxed);
        "Unknown"
    }
    .to_string()
}

fn is_method_call_statement(tokens: &[String]) -> bool {
    let arrow_idx = tokens
        .iter()
        .position(|x| x == "->" || x == "=>")
        .unwrap_or(usize::MAX);
    if arrow_idx == usize::MAX {
        return false;
    }
    let eq_idx = tokens.iter().position(|x| x == "=").unwrap_or(usize::MAX);
    arrow_idx < eq_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_method_call_with_named_arg() {
        let tokens = tokenize_statement("me->run( iv_input = lv_greeting ).");
        assert_eq!(
            tokens,
            vec![
                "me",
                "->",
                "run",
                "(",
                "iv_input",
                "=",
                "lv_greeting",
                ")",
                "."
            ]
        );
        assert_eq!(classify_statement(&tokens), "Call");
    }

    #[test]
    fn tokenizes_method_signature() {
        let tokens = tokenize_statement("METHODS greet RETURNING VALUE(rv_result) TYPE string.");
        assert_eq!(
            tokens,
            vec![
                "METHODS",
                "greet",
                "RETURNING",
                "VALUE",
                "(",
                "rv_result",
                ")",
                "TYPE",
                "string",
                "."
            ]
        );
        assert_eq!(classify_statement(&tokens), "MethodDef");
    }

    #[test]
    fn splits_multiline_class() {
        let program = generate_source(
            "CLASS z DEFINITION PUBLIC.\n  PUBLIC SECTION.\n    METHODS run.\nENDCLASS.\n",
            "z.clas.abap",
            "CLAS",
        );
        assert_eq!(program.statements.len(), 4);
        assert_eq!(program.statements[0].kind, "ClassDefinition");
        assert_eq!(program.statements[2].kind, "MethodDef");
    }

    #[test]
    fn classifies_control_flow_keywords() {
        let cases = [
            ("IF lv_x = 1.", "If"),
            ("ELSEIF lv_x = 2.", "ElseIf"),
            ("ELSE.", "Else"),
            ("ENDIF.", "EndIf"),
            ("CASE lv_x.", "Case"),
            ("WHEN 1.", "When"),
            ("WHEN OTHERS.", "WhenOthers"),
            ("ENDCASE.", "EndCase"),
            ("WHILE lv_x < 10.", "While"),
            ("ENDWHILE.", "EndWhile"),
            ("DO 5 TIMES.", "Do"),
            ("ENDDO.", "EndDo"),
            ("LOOP AT lt_tab INTO ls_row.", "Loop"),
            ("ENDLOOP.", "EndLoop"),
            ("TRY.", "Try"),
            ("CATCH cx_root INTO lx_err.", "Catch"),
            ("CLEANUP.", "Cleanup"),
            ("ENDTRY.", "EndTry"),
            ("CHECK lv_x > 0.", "Check"),
            ("EXIT.", "Exit"),
            ("CONTINUE.", "Continue"),
            ("RETURN.", "Return"),
            ("RAISE EXCEPTION TYPE cx_root.", "Raise"),
        ];
        for (source, expected) in cases {
            let tokens = tokenize_statement(source);
            assert_eq!(classify_statement(&tokens), expected, "source: {source}");
        }
    }

    #[test]
    fn classifies_file_system_and_security_keywords() {
        let cases = [
            (
                "OPEN DATASET lv_file FOR INPUT IN TEXT MODE.",
                "OpenDataset",
            ),
            ("READ DATASET lv_file INTO lv_line.", "ReadDataset"),
            ("DELETE DATASET lv_file.", "DeleteDataset"),
            ("DELETE DYNPRO lv_program 100.", "Unknown"),
            ("TRANSFER lv_line TO lv_file.", "Transfer"),
            (
                "AUTHORITY-CHECK OBJECT 'S_TCODE' ID 'TCD' FIELD lv_tcode.",
                "AuthorityCheck",
            ),
            (
                "GENERATE SUBROUTINE POOL lv_code NAME lv_prog.",
                "GenerateSubroutine",
            ),
            (
                "CALL TRANSFORMATION 'ID' SOURCE text = lv_line RESULT XML lv_xml.",
                "Unknown",
            ),
            ("EDITOR-CALL FOR REPORT lv_prog.", "EditorCall"),
        ];
        for (source, expected) in cases {
            let tokens = tokenize_statement(source);
            assert_eq!(classify_statement(&tokens), expected, "source: {source}");
        }
    }

    #[test]
    fn control_flow_is_lowercase_insensitive() {
        let tokens = tokenize_statement("if lv_x = 1.");
        assert_eq!(classify_statement(&tokens), "If");
    }

    #[test]
    fn emits_full_line_comments() {
        let program = generate_source("* hello\nCLASS z DEFINITION PUBLIC.\n", "x.abap", "PROG");
        assert_eq!(program.statements[0].kind, "Comment");
        assert_eq!(program.statements[0].tokens[0].str, "* hello");
        assert_eq!(program.statements[0].start, Position { row: 1, col: 1 });
        assert_eq!(program.statements[0].end, Position { row: 1, col: 8 });
    }

    #[test]
    fn unknown_statement_increments_counter() {
        let before = unclassified_count();
        // A bare keyword the classifier does not recognise and which has no '='.
        let tokens = tokenize_statement("COMMIT WORK.");
        assert_eq!(classify_statement(&tokens), "Unknown");
        assert!(unclassified_count() > before);
    }

    #[test]
    fn serializes_expected_shape() {
        let program = generate_source(
            "FORM run.\n  DATA lv TYPE string.\nENDFORM.\n",
            "x.abap",
            "PROG",
        );
        let json = serde_json::to_value(program).expect("json");
        assert_eq!(json["file"], "x.abap");
        assert_eq!(json["objectType"], "PROG");
        assert_eq!(json["statements"][0]["type"], "Form");
        assert_eq!(json["statements"][1]["tokens"][0]["str"], "DATA");
    }
}
