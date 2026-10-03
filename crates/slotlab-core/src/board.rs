//! A game's entry board — the `INITIAL_BOARD` of `src/game/constants.ts` —
//! read from the TypeScript source and written back into it.
//!
//! The file is source code, not data: the board is an array literal that may
//! reference named symbol constants declared above it
//! (`const SHOWCASE_SHARK: RawSymbol = { name: 'W', multiplier: 25 }`), and may
//! carry comments, trailing commas and either quote style. Reading therefore
//! PARSES the literal (a small JS-literal parser, not a regex), and writing
//! replaces ONLY the declaration's own byte range: the comments above it, the
//! constants it uses and everything else in the file stay byte for byte.
//!
//! ⚠️ A constant is only reusable by name if it is declared BEFORE the board: a
//! `const` read above its declaration throws at module load (temporal dead
//! zone), so the writer never emits a name the reader could not have resolved.

use serde_json::{Map, Number, Value};
use std::collections::HashMap;

/// One symbol of the board, keys in source order (`name` first).
pub type Cell = Vec<(String, Value)>;

#[derive(Debug, Clone)]
pub struct ParsedBoard {
    /// `board[reel][row]`, padding rows included (first and last of each reel).
    pub board: Vec<Vec<Cell>>,
    /// Object constants declared before the board — the names it may use.
    pub consts: Vec<(String, Cell)>,
    /// Byte range of the declaration, from the start of its line to its `;`.
    pub start: usize,
    pub end: usize,
    /// Indentation of the declaration line itself (empty at top level).
    pub decl_indent: String,
    /// Type annotation as written, colon included (`: RawSymbol[][]`), or "".
    pub annotation: String,
    /// Indentation unit of one reel line, as written (`\t` by default).
    pub indent: String,
    /// Quote used for strings (`'` by default).
    pub quote: char,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BoardError {
    #[error("no `export const INITIAL_BOARD` in the file")]
    NotFound,
    #[error("INITIAL_BOARD: {0}")]
    Syntax(String),
    #[error("INITIAL_BOARD: {0}")]
    Shape(String),
}

// ---------------------------------------------------------------- parsing --

#[derive(Debug, Clone)]
enum Lit {
    Obj(Vec<(String, Lit)>),
    Arr(Vec<Lit>),
    Val(Value),
    Ident(String),
}

struct Parser<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, at: usize) -> Self {
        Self {
            src,
            b: src.as_bytes(),
            i: at,
        }
    }

    fn err<T>(&self, msg: &str) -> Result<T, BoardError> {
        let at = self.i.min(self.src.len());
        let line = self.src[..at].matches('\n').count() + 1;
        Err(BoardError::Syntax(format!("{msg} (line {line})")))
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn starts(&self, s: &str) -> bool {
        self.b[self.i.min(self.b.len())..].starts_with(s.as_bytes())
    }

    /// Whitespace and comments.
    fn ws(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if self.starts("//") {
                while self.peek().is_some_and(|c| c != b'\n') {
                    self.i += 1;
                }
            } else if self.starts("/*") {
                match self.src[self.i + 2..].find("*/") {
                    Some(k) => self.i += 2 + k + 2,
                    None => self.i = self.b.len(),
                }
            } else {
                return;
            }
        }
    }

    fn ident(&mut self) -> &'a str {
        let s = self.i;
        while self.peek().is_some_and(is_ident_char) {
            self.i += 1;
        }
        &self.src[s..self.i]
    }

    fn value(&mut self) -> Result<Lit, BoardError> {
        self.ws();
        match self.peek() {
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'\'' | b'"' | b'`') => Ok(Lit::Val(Value::String(self.string()?))),
            Some(b'.') if self.starts("...") => self.err("spread (...) is not supported"),
            Some(c) if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() => self.number(),
            Some(c) if is_ident_start(c) => {
                let id = self.ident();
                match id {
                    "true" => Ok(Lit::Val(Value::Bool(true))),
                    "false" => Ok(Lit::Val(Value::Bool(false))),
                    "null" => Ok(Lit::Val(Value::Null)),
                    _ => {
                        self.ws();
                        match self.peek() {
                            Some(b'.') | Some(b'[') => {
                                self.err(&format!("`{id}.…` member access is not supported"))
                            }
                            Some(b'(') => self.err(&format!("`{id}(…)` calls are not supported")),
                            _ => Ok(Lit::Ident(id.to_string())),
                        }
                    }
                }
            }
            Some(c) => self.err(&format!("unexpected character `{}`", c as char)),
            None => self.err("unexpected end of file"),
        }
    }

    fn array(&mut self) -> Result<Lit, BoardError> {
        self.i += 1; // [
        let mut items = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some(b']') {
                self.i += 1;
                return Ok(Lit::Arr(items));
            }
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Lit::Arr(items));
                }
                _ => return self.err("expected `,` or `]`"),
            }
        }
    }

    fn object(&mut self) -> Result<Lit, BoardError> {
        self.i += 1; // {
        let mut fields = Vec::new();
        loop {
            self.ws();
            let key = match self.peek() {
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Lit::Obj(fields));
                }
                Some(b'.') if self.starts("...") => return self.err("spread (...) is not supported"),
                Some(b'\'' | b'"') => self.string()?,
                Some(c) if is_ident_start(c) => self.ident().to_string(),
                Some(c) if c.is_ascii_digit() => {
                    let s = self.i;
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        self.i += 1;
                    }
                    self.src[s..self.i].to_string()
                }
                _ => return self.err("expected a property name"),
            };
            self.ws();
            if self.peek() != Some(b':') {
                return self.err(&format!("`{key}`: shorthand properties are not supported"));
            }
            self.i += 1;
            let v = self.value()?;
            fields.push((key, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Lit::Obj(fields));
                }
                _ => return self.err("expected `,` or `}`"),
            }
        }
    }

    fn string(&mut self) -> Result<String, BoardError> {
        let q = self.b[self.i];
        self.i += 1;
        let mut out = String::new();
        loop {
            let Some(rest) = self.src.get(self.i..) else {
                return self.err("unterminated string");
            };
            let Some(ch) = rest.chars().next() else {
                return self.err("unterminated string");
            };
            self.i += ch.len_utf8();
            match ch {
                c if c as u32 == q as u32 => return Ok(out),
                '$' if q == b'`' && self.peek() == Some(b'{') => {
                    return self.err("template interpolation is not supported")
                }
                '\\' => {
                    let Some(e) = self.src[self.i..].chars().next() else {
                        return self.err("unterminated string");
                    };
                    self.i += e.len_utf8();
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '0' => out.push('\0'),
                        'u' => {
                            let hex: String = if self.peek() == Some(b'{') {
                                let end = self.src[self.i..].find('}').unwrap_or(0);
                                let h = self.src[self.i + 1..self.i + end].to_string();
                                self.i += end + 1;
                                h
                            } else {
                                let h = self.src.get(self.i..self.i + 4).unwrap_or("").to_string();
                                self.i += 4;
                                h
                            };
                            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                Some(c) => out.push(c),
                                None => return self.err("bad \\u escape"),
                            }
                        }
                        '\n' => {}
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn number(&mut self) -> Result<Lit, BoardError> {
        let s = self.i;
        if matches!(self.peek(), Some(b'-' | b'+')) {
            self.i += 1;
        }
        if self.starts("0x") || self.starts("0X") {
            self.i += 2;
            let h = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit() || c == b'_') {
                self.i += 1;
            }
            let digits = self.src[h..self.i].replace('_', "");
            let neg = self.src[s..h].starts_with('-');
            return match i64::from_str_radix(&digits, 16) {
                Ok(n) => Ok(Lit::Val(Value::Number(Number::from(if neg { -n } else { n })))),
                Err(_) => self.err("bad hexadecimal number"),
            };
        }
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'e' | b'E'))
        {
            let c = self.b[self.i];
            self.i += 1;
            if matches!(c, b'e' | b'E') && matches!(self.peek(), Some(b'-' | b'+')) {
                self.i += 1;
            }
        }
        let text = self.src[s..self.i].replace('_', "");
        let text = text.trim_start_matches('+');
        if !text.contains(|c| matches!(c, '.' | 'e' | 'E')) {
            if let Ok(n) = text.parse::<i64>() {
                return Ok(Lit::Val(Value::Number(Number::from(n))));
            }
        }
        match text.parse::<f64>().ok().and_then(Number::from_f64) {
            Some(n) => Ok(Lit::Val(Value::Number(n))),
            None => self.err(&format!("bad number `{text}`")),
        }
    }
}

fn to_value(l: &Lit) -> Result<Value, String> {
    Ok(match l {
        Lit::Val(v) => v.clone(),
        Lit::Arr(a) => Value::Array(a.iter().map(to_value).collect::<Result<_, _>>()?),
        Lit::Obj(f) => {
            let mut m = Map::new();
            for (k, v) in f {
                m.insert(k.clone(), to_value(v)?);
            }
            Value::Object(m)
        }
        Lit::Ident(id) => return Err(format!("`{id}` inside a symbol is not supported")),
    })
}

fn to_cell(fields: &[(String, Lit)]) -> Result<Cell, String> {
    fields
        .iter()
        .map(|(k, v)| Ok((k.clone(), to_value(v)?)))
        .collect()
}

/// `const NAME[: Type] = { … }` declared before `before`, in file order.
fn object_consts(src: &str, before: usize) -> Vec<(String, Cell)> {
    let re = regex::Regex::new(r"\bconst\s+([A-Za-z_$][\w$]*)\s*(?::[^=;]+)?=\s*\{").unwrap();
    let mut out = Vec::new();
    for m in re.captures_iter(src) {
        let whole = m.get(0).unwrap();
        if whole.start() >= before {
            break;
        }
        let mut p = Parser::new(src, whole.end() - 1);
        if let Ok(Lit::Obj(fields)) = p.object() {
            if let Ok(cell) = to_cell(&fields) {
                out.push((m[1].to_string(), cell));
            }
        }
    }
    out
}

/// Reads the `INITIAL_BOARD` declaration of a `constants.ts`.
pub fn parse(src: &str) -> Result<ParsedBoard, BoardError> {
    let re = regex::Regex::new(r"(?m)^([ \t]*)export\s+const\s+INITIAL_BOARD\b").unwrap();
    let caps = re.captures(src).ok_or(BoardError::NotFound)?;
    let decl = caps.get(0).unwrap();
    let decl_indent = caps[1].to_string();

    let mut p = Parser::new(src, decl.end());
    p.ws();
    let mut annotation = String::new();
    if p.peek() == Some(b':') {
        let eq = src[p.i..]
            .find('=')
            .ok_or_else(|| BoardError::Syntax("no `=` after the type".into()))?;
        annotation = format!(": {}", src[p.i + 1..p.i + eq].trim());
        p.i += eq;
    }
    if p.peek() != Some(b'=') {
        return p.err("expected `=`");
    }
    p.i += 1;
    p.ws();
    if p.peek() != Some(b'[') {
        return p.err("the board must be an array literal `[ … ]`");
    }
    let open = p.i;
    let lit = p.value()?;
    p.ws();
    if p.starts("as ") || p.starts("satisfies ") {
        while p.peek().is_some_and(|c| c != b';' && c != b'\n') {
            p.i += 1;
        }
    }
    if p.peek() == Some(b';') {
        p.i += 1;
    }
    let end = p.i;
    let start = decl.start();

    let consts = object_consts(src, start);
    let Lit::Arr(reels) = lit else {
        unreachable!("checked `[` above")
    };
    let mut board = Vec::with_capacity(reels.len());
    for (r, reel) in reels.iter().enumerate() {
        let Lit::Arr(items) = reel else {
            return Err(BoardError::Shape(format!("reel {} is not an array", r + 1)));
        };
        let mut cells = Vec::with_capacity(items.len());
        for (row, item) in items.iter().enumerate() {
            let cell = match item {
                Lit::Obj(fields) => to_cell(fields).map_err(BoardError::Shape)?,
                Lit::Ident(id) => match consts.iter().find(|(n, _)| n == id) {
                    Some((_, c)) => c.clone(),
                    None => {
                        return Err(BoardError::Shape(format!(
                            "`{id}` (reel {}, row {row}) is not an object constant declared above the board",
                            r + 1
                        )))
                    }
                },
                _ => {
                    return Err(BoardError::Shape(format!(
                        "reel {}, row {row}: expected a symbol object",
                        r + 1
                    )))
                }
            };
            if !cell.iter().any(|(k, v)| k == "name" && v.is_string()) {
                return Err(BoardError::Shape(format!(
                    "reel {}, row {row}: symbol without a `name`",
                    r + 1
                )));
            }
            cells.push(cell);
        }
        board.push(cells);
    }

    // Style of the block as written, so a rewrite reads like the original.
    let block = &src[open..end];
    let indent = block
        .split('\n')
        .nth(1)
        .map(|l| l[..l.len() - l.trim_start().len()].to_string())
        .map(|w| w.strip_prefix(decl_indent.as_str()).unwrap_or(&w).to_string())
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| "\t".to_string());
    let quote = block
        .chars()
        .find(|&c| c == '\'' || c == '"')
        .unwrap_or('\'');

    Ok(ParsedBoard {
        board,
        consts,
        start,
        end,
        decl_indent,
        annotation,
        indent,
        quote,
    })
}

// ---------------------------------------------------------------- writing --

fn is_ident(k: &str) -> bool {
    let b = k.as_bytes();
    !b.is_empty() && is_ident_start(b[0]) && b.iter().all(|&c| is_ident_char(c))
}

fn quote_str(s: &str, q: char) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

fn render_value(v: &Value, q: char) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => quote_str(s, q),
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(|x| render_value(x, q)).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(m) if m.is_empty() => "{}".into(),
        Value::Object(m) => format!(
            "{{ {} }}",
            m.iter()
                .map(|(k, x)| format!("{}: {}", render_key(k, q), render_value(x, q)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn render_key(k: &str, q: char) -> String {
    if is_ident(k) {
        k.to_string()
    } else {
        quote_str(k, q)
    }
}

/// Same symbol, whatever the key order.
pub fn cell_eq(a: &Cell, b: &Cell) -> bool {
    a.len() == b.len()
        && a
            .iter()
            .all(|(k, v)| b.iter().any(|(k2, v2)| k2 == k && v2 == v))
}

fn render_cell(cell: &Cell, consts: &[(String, Cell)], q: char) -> String {
    if let Some((name, _)) = consts.iter().find(|(_, c)| cell_eq(c, cell)) {
        return name.clone();
    }
    if cell.is_empty() {
        return "{}".into();
    }
    format!(
        "{{ {} }}",
        cell.iter()
            .map(|(k, v)| format!("{}: {}", render_key(k, q), render_value(v, q)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The declaration for `board`, in the style of `parsed` (one reel per line).
pub fn render(board: &[Vec<Cell>], parsed: &ParsedBoard) -> String {
    let d = &parsed.decl_indent;
    let mut out = format!("{d}export const INITIAL_BOARD{} = [\n", parsed.annotation);
    for reel in board {
        let cells: Vec<String> = reel
            .iter()
            .map(|c| render_cell(c, &parsed.consts, parsed.quote))
            .collect();
        out.push_str(&format!("{d}{}[{}],\n", parsed.indent, cells.join(", ")));
    }
    out.push_str(&format!("{d}];"));
    out
}

/// Key order per symbol name, as the file writes it — so a board coming back
/// from JSON (whose objects have lost their order) is written the same way.
pub fn key_order_hints(parsed: &ParsedBoard) -> HashMap<String, Vec<String>> {
    let mut hints: HashMap<String, Vec<String>> = HashMap::new();
    let all = parsed
        .consts
        .iter()
        .map(|(_, c)| c)
        .chain(parsed.board.iter().flatten());
    for cell in all {
        let Some(name) = cell.iter().find(|(k, _)| k == "name").and_then(|(_, v)| v.as_str())
        else {
            continue;
        };
        let keys = hints.entry(name.to_string()).or_default();
        for (k, _) in cell {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
    }
    hints
}

/// A JSON symbol as a [`Cell`]: `name` first, then the keys in the order the
/// file already uses for that symbol, then any new key alphabetically.
pub fn cell_from_map(map: &Map<String, Value>, hints: &HashMap<String, Vec<String>>) -> Cell {
    let name = map.get("name").and_then(Value::as_str).unwrap_or_default();
    let order = hints.get(name);
    let rank = |k: &str| -> (usize, usize) {
        if k == "name" {
            return (0, 0);
        }
        match order.and_then(|o| o.iter().position(|x| x == k)) {
            Some(p) => (1, p),
            None => (2, 0),
        }
    };
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    keys.into_iter()
        .map(|k| (k.clone(), map[k].clone()))
        .collect()
}

/// `src` with its `INITIAL_BOARD` replaced by `board`. Adds `// prettier-ignore`
/// above the declaration when missing: one reel per line keeps the grid legible,
/// and Prettier would otherwise explode it into one symbol per line.
pub fn replace(src: &str, board: &[Vec<Cell>]) -> Result<String, BoardError> {
    let parsed = parse(src)?;
    let head = &src[..parsed.start];
    let tail = &src[parsed.end..];
    let prev_line = head
        .trim_end_matches(|c: char| c.is_whitespace())
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim();
    let mut out = String::with_capacity(src.len() + 256);
    out.push_str(head);
    if prev_line != "// prettier-ignore" {
        out.push_str(&parsed.decl_indent);
        out.push_str("// prettier-ignore\n");
    }
    out.push_str(&render(board, &parsed));
    out.push_str(tail);
    Ok(out)
}

/// The board as JSON objects (what the UI edits).
pub fn board_to_json(board: &[Vec<Cell>]) -> Vec<Vec<Map<String, Value>>> {
    board
        .iter()
        .map(|reel| {
            reel.iter()
                .map(|c| c.iter().cloned().collect::<Map<String, Value>>())
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOODWATER: &str = r#"import type { RawSymbol } from './types';

/**
 * Plateau d'accueil — commentaire qui doit survivre.
 */
const SHOWCASE_SHARK: RawSymbol = { name: 'W', wild: true, scatter: true, multiplier: 25 };
// Un rouleau par ligne.
// prettier-ignore
export const INITIAL_BOARD: RawSymbol[][] = [
	[{ name: 'L4' }, { name: 'H1' }, { name: 'L1' }, { name: 'L3' }, { name: 'L5' }, { name: 'L2' }],
	[{ name: 'L5' }, { name: 'H2' }, { name: 'H5' }, { name: 'H4' }, { name: 'L4' }, { name: 'L5' }], // fin
	[{ name: 'L4' }, { name: 'H3' }, SHOWCASE_SHARK, { name: 'M', value: 2 }, { name: 'L2' }, { name: 'L3' }],
];

export const BOARD_DIMENSIONS = { x: INITIAL_BOARD.length, y: INITIAL_BOARD[0].length - 2 };
"#;

    const PRETTIER: &str = "export const INITIAL_BOARD: RawSymbol[][] = [\n\t[\n\t\t{ name: \"H3\" },\n\t\t{ name: \"H1\", multiplier: 2.5 },\n\t],\n\t[\n\t\t{ name: \"L1\" },\n\t\t{ name: \"L2\" },\n\t],\n];\n";

    fn name(c: &Cell) -> &str {
        c.iter().find(|(k, _)| k == "name").unwrap().1.as_str().unwrap()
    }

    #[test]
    fn parses_constants_and_comments() {
        let p = parse(BLOODWATER).unwrap();
        assert_eq!(p.board.len(), 3);
        assert!(p.board.iter().all(|r| r.len() == 6));
        let shark = &p.board[2][2];
        assert_eq!(name(shark), "W");
        assert!(shark.contains(&("multiplier".into(), Value::from(25))));
        assert_eq!(p.board[2][3], vec![("name".into(), Value::from("M")), ("value".into(), Value::from(2))]);
        assert_eq!(p.annotation, ": RawSymbol[][]");
        assert_eq!(p.indent, "\t");
        assert_eq!(p.quote, '\'');
        assert_eq!(p.consts.len(), 1);
    }

    #[test]
    fn rewrite_keeps_the_file_and_reuses_constants() {
        let p = parse(BLOODWATER).unwrap();
        let mut board = p.board.clone();
        board[0][1] = vec![("name".into(), Value::from("H2"))];
        let out = replace(BLOODWATER, &board).unwrap();
        assert!(out.contains("commentaire qui doit survivre"));
        assert!(out.contains("export const BOARD_DIMENSIONS"));
        assert!(out.contains("{ name: 'L4' }, { name: 'H3' }, SHOWCASE_SHARK, { name: 'M', value: 2 }"));
        assert!(out.contains("[{ name: 'L4' }, { name: 'H2' }, { name: 'L1' }"));
        assert_eq!(out.matches("// prettier-ignore").count(), 1, "no duplicate ignore");
        let again = parse(&out).unwrap();
        assert_eq!(again.board, board);
        // Writing the same board twice is a fixed point.
        assert_eq!(replace(&out, &board).unwrap(), out);
    }

    #[test]
    fn prettier_layout_is_read_and_rewritten_one_reel_per_line() {
        let p = parse(PRETTIER).unwrap();
        assert_eq!(p.quote, '"');
        assert_eq!(p.board[0][1], vec![("name".into(), Value::from("H1")), ("multiplier".into(), Value::from(2.5))]);
        let out = replace(PRETTIER, &p.board).unwrap();
        assert!(out.starts_with("// prettier-ignore\nexport const INITIAL_BOARD: RawSymbol[][] = [\n\t[{ name: \"H3\" }, { name: \"H1\", multiplier: 2.5 }],\n"));
        assert_eq!(parse(&out).unwrap().board, p.board);
    }

    #[test]
    fn json_cells_follow_the_file_key_order() {
        let p = parse(BLOODWATER).unwrap();
        let hints = key_order_hints(&p);
        let mut m = Map::new();
        m.insert("multiplier".into(), Value::from(50));
        m.insert("scatter".into(), Value::from(true));
        m.insert("wild".into(), Value::from(true));
        m.insert("name".into(), Value::from("W"));
        let keys: Vec<String> = cell_from_map(&m, &hints).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["name", "wild", "scatter", "multiplier"]);
    }

    #[test]
    fn refuses_what_it_cannot_round_trip() {
        assert_eq!(parse("const A = 1;").unwrap_err(), BoardError::NotFound);
        assert!(matches!(
            parse("export const INITIAL_BOARD = [[...X]];"),
            Err(BoardError::Syntax(_))
        ));
        // A constant declared AFTER the board is not usable (temporal dead zone).
        let late = "export const INITIAL_BOARD = [[S]];\nconst S = { name: 'W' };";
        assert!(matches!(parse(late), Err(BoardError::Shape(_))));
        assert!(matches!(
            parse("export const INITIAL_BOARD = [[{ value: 2 }]];"),
            Err(BoardError::Shape(_))
        ));
    }
}
