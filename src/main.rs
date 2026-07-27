//! apolisp — a small Lisp in the Clojure dialect with its own VM.
//!
//! One file with inline `mod` blocks (ADR-015). The module paths are the ones
//! chosen up front, so extraction into files later is a move rather than a
//! redesign. The seams are for subtraction: each `mod` below should be cuttable
//! or liftable, not merely a home for code.
//!
//! Milestone 1 (BUILD.md): reader + printer + forms with span origins.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: apolisp <read|spans|sizes|expand|compile|run> [file.xs]");
        return ExitCode::from(2);
    }
    if args[1] == "sizes" {
        return value::report_sizes();
    }
    if args.len() < 3 {
        eprintln!("usage: apolisp <read|spans|expand|compile|run> <file.xs>");
        return ExitCode::from(2);
    }
    let (cmd, path) = (args[1].as_str(), args[2].as_str());

    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("apolisp: {path}: {e}");
            return ExitCode::from(2);
        }
    };

    match cmd {
        "read" => {
            let mut interner = value::Interner::new();
            match reader::read_all(&src, &mut interner) {
                Ok(forms) => {
                    for f in &forms {
                        println!("{}", printer::print(&f.root, &interner));
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.render(path, &src));
                    ExitCode::FAILURE
                }
            }
        }
        // Debug views. These exist because ADR-026 puts origins outside the
        // value graph, so the ordinary printed form cannot show them — and an
        // invariant nobody can see is one nobody checks.
        "spans" => {
            let mut interner = value::Interner::new();
            match reader::read_all(&src, &mut interner) {
                Ok(forms) => {
                    let mut problems = Vec::new();
                    for f in &forms {
                        value::check_origins(&f.root, &f.origins, &src, &mut problems);
                    }
                    for f in &forms {
                        println!("{}", value::print_origins(&f.root, &f.origins, &interner, 0));
                    }
                    if problems.is_empty() {
                        println!("ok: span invariants hold");
                        ExitCode::SUCCESS
                    } else {
                        for p in &problems {
                            println!("VIOLATION: {p}");
                        }
                        ExitCode::FAILURE
                    }
                }
                Err(e) => {
                    eprintln!("{}", e.render(path, &src));
                    ExitCode::FAILURE
                }
            }
        }
        // Stages whose milestone has not landed. They fail rather than no-op:
        // a smoke test that silently skips a stage stops being an oracle.
        "expand" | "compile" | "run" => {
            eprintln!("apolisp: `{cmd}` is not implemented yet (see BUILD.md)");
            ExitCode::FAILURE
        }
        _ => {
            eprintln!("apolisp: unknown command `{cmd}`");
            ExitCode::from(2)
        }
    }
}

// ---------------------------------------------------------------------------

/// Source positions and reader errors.
///
/// Spans are byte offsets into one source string; line and column are derived
/// only when an error is rendered. Storing both would be two things to keep in
/// agreement, and only one of them is needed on the hot path.
pub mod error {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct Span {
        pub start: u32,
        pub end: u32,
    }

    impl Span {
        pub fn new(start: usize, end: usize) -> Span {
            Span { start: start as u32, end: end as u32 }
        }
    }

    /// Where a span came from (ADR-026).
    ///
    /// `Generated` carries the macro call site rather than a position in any
    /// file. `Unknown` prints as unknown — the point of naming it is that span
    /// loss stays visible instead of degrading into a plausible lie.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum SpanOrigin {
        Source(Span),
        Generated(Span),
        Unknown,
    }

    impl SpanOrigin {
        pub fn span(&self) -> Option<Span> {
            match self {
                SpanOrigin::Source(s) | SpanOrigin::Generated(s) => Some(*s),
                SpanOrigin::Unknown => None,
            }
        }
    }

    #[derive(Debug)]
    pub struct LispErr {
        pub msg: String,
        pub origin: SpanOrigin,
    }

    impl LispErr {
        pub fn at(span: Span, msg: impl Into<String>) -> LispErr {
            LispErr { msg: msg.into(), origin: SpanOrigin::Source(span) }
        }

        /// Render with 1-based line and column, resolved against the source
        /// the span came from.
        pub fn render(&self, path: &str, src: &str) -> String {
            match self.origin.span() {
                Some(s) => {
                    let (line, col) = line_col(src, s.start as usize);
                    format!("{path}:{line}:{col}: {}", self.msg)
                }
                None => format!("{path}: {}", self.msg),
            }
        }
    }

    pub fn line_col(src: &str, offset: usize) -> (usize, usize) {
        let mut line = 1;
        let mut line_start = 0;
        for (i, b) in src.bytes().enumerate() {
            if i >= offset {
                break;
            }
            if b == b'\n' {
                line += 1;
                line_start = i + 1;
            }
        }
        // Column counts characters, not bytes, so a multi-byte character does
        // not report a column past the end of its own line.
        let col = src[line_start..offset.min(src.len())].chars().count() + 1;
        (line, col)
    }
}

// ---------------------------------------------------------------------------

/// Values. Forms *are* values (ADR-023); what makes a value a form is the span
/// origins travelling beside it, not its representation.
pub mod value {
    use crate::error::SpanOrigin;
    use std::collections::HashMap;
    use std::rc::Rc;

    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct SymId(pub u32);
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct KwId(pub u32);
    // Arena indices, not pointers (ADR-025). Unused until their milestone, but
    // present so `size_of::<Value>()` is asserted against the real enum.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct CellId(pub u32, pub u32);
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct HandleId(pub u32, pub u32);
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct BufferId(pub u32, pub u32);

    #[derive(Debug)]
    pub struct StrObj(pub String);
    #[derive(Debug)]
    pub struct BytesObj(pub Vec<u8>);
    #[derive(Debug)]
    pub struct ListObj(pub Vec<Value>);
    #[derive(Debug)]
    pub struct VecObj(pub Vec<Value>);
    /// Insertion-ordered pairs. Q6 owns the real representation; this one is
    /// provisional and deliberately the dumbest thing that reads back in the
    /// order it was written, because iteration order reaches golden output and
    /// nondeterminism there kills the oracle (BUILD.md).
    #[derive(Debug)]
    pub struct MapObj(pub Vec<(Value, Value)>);
    #[derive(Debug)]
    pub struct Closure;

    /// ADR-025. `PartialEq` is implemented by hand below, never derived —
    /// derived equality follows Rust variant and payload equality, which is the
    /// wrong answer for a language whose `=` crosses representations
    /// (`TRAPS.md`).
    #[derive(Clone, Debug)]
    pub enum Value {
        Nil,
        Bool(bool),
        Int(i64),
        Float(f64),
        Str(Rc<StrObj>),
        Bytes(Rc<BytesObj>),
        Sym(SymId),
        Keyword(KwId),
        List(Rc<ListObj>),
        Vec(Rc<VecObj>),
        Map(Rc<MapObj>),
        Fn(Rc<Closure>),
        Cell(CellId),
        Handle(HandleId),
        Buffer(BufferId),
    }

    impl PartialEq for Value {
        fn eq(&self, other: &Value) -> bool {
            use Value::*;
            match (self, other) {
                (Nil, Nil) => true,
                (Bool(a), Bool(b)) => a == b,
                (Int(a), Int(b)) => a == b,
                // Q13 owns numeric equality: whether 1 == 1.0, and NaN/-0.0.
                // Until it is decided, floats compare by IEEE rules and never
                // to integers, which is the option that is easy to widen later.
                (Float(a), Float(b)) => a == b,
                (Str(a), Str(b)) => a.0 == b.0,
                (Bytes(a), Bytes(b)) => a.0 == b.0,
                (Sym(a), Sym(b)) => a == b,
                (Keyword(a), Keyword(b)) => a == b,
                (List(a), List(b)) => a.0 == b.0,
                (Vec(a), Vec(b)) => a.0 == b.0,
                (Map(a), Map(b)) => a.0 == b.0,
                // Q20: Clojure makes '(1 2) equal [1 2]. Cross-type sequential
                // equality is deliberately absent until that question is
                // settled — widening equality later is safe, narrowing is not.
                _ => false,
            }
        }
    }

    /// One table for symbols and keywords. They are distinct `Value` variants
    /// (ADR-025) but share interning, so an id alone does not say which kind
    /// you have — that is a `TRAPS.md` entry, and the reason the variants are
    /// separate rather than a flag bit.
    pub struct Interner {
        names: Vec<String>,
        index: HashMap<String, u32>,
    }

    impl Interner {
        pub fn new() -> Interner {
            Interner { names: Vec::new(), index: HashMap::new() }
        }

        pub fn intern(&mut self, name: &str) -> u32 {
            if let Some(&id) = self.index.get(name) {
                return id;
            }
            let id = self.names.len() as u32;
            self.names.push(name.to_string());
            self.index.insert(name.to_string(), id);
            id
        }

        pub fn sym(&mut self, name: &str) -> Value {
            Value::Sym(SymId(self.intern(name)))
        }

        pub fn keyword(&mut self, name: &str) -> Value {
            Value::Keyword(KwId(self.intern(name)))
        }

        pub fn name(&self, id: u32) -> &str {
            &self.names[id as usize]
        }

        pub fn len(&self) -> usize {
            self.names.len()
        }
    }

    impl Default for Interner {
        fn default() -> Interner {
            Interner::new()
        }
    }

    /// The span carrier (ADR-026). Origins live outside the value graph and
    /// mirror its shape: one entry per syntactic child, so immediates are
    /// covered by the same mechanism as heap objects.
    ///
    /// A map contributes two children per pair, key then value.
    #[derive(Clone, Debug)]
    pub struct Origins {
        pub origin: SpanOrigin,
        pub children: Vec<Origins>,
    }

    impl Origins {
        pub fn leaf(origin: SpanOrigin) -> Origins {
            Origins { origin, children: Vec::new() }
        }
    }

    /// A value the reader produced, or that the expander is treating as code.
    #[derive(Clone, Debug)]
    pub struct LocatedForm {
        pub root: Value,
        pub origins: Origins,
    }

    /// How many syntactic children a value has, for checking that an `Origins`
    /// tree still lines up with the value it describes.
    pub fn child_count(v: &Value) -> usize {
        match v {
            Value::List(l) => l.0.len(),
            Value::Vec(v) => v.0.len(),
            Value::Map(m) => m.0.len() * 2,
            _ => 0,
        }
    }

    /// Walk a value against its origins and collect every way they disagree.
    ///
    /// Three invariants (ADR-026): a `Source` span lies inside the file it came
    /// from, both of its ends fall on character boundaries, and child-origin
    /// arity matches child count. The arity check is the one that catches a
    /// whole *category* of node silently having no origin, which is the failure
    /// `../reg-lisp` hit and its suite missed.
    ///
    /// The boundary check earns its place because the alternative to reporting
    /// a misaligned span is panicking inside error rendering — a reader bug
    /// then presents as a crash on the input it was meant to reject.
    pub fn check_origins(v: &Value, o: &Origins, src: &str, out: &mut Vec<String>) {
        if let SpanOrigin::Source(s) = o.origin {
            let (start, end) = (s.start as usize, s.end as usize);
            if s.start > s.end || end > src.len() {
                out.push(format!(
                    "span {}..{} outside source of {} bytes",
                    s.start,
                    s.end,
                    src.len()
                ));
            } else if !src.is_char_boundary(start) || !src.is_char_boundary(end) {
                out.push(format!(
                    "span {}..{} does not lie on character boundaries",
                    s.start, s.end
                ));
            }
        }
        let want = child_count(v);
        if o.children.len() != want {
            out.push(format!(
                "{} has {want} syntactic children but {} origins",
                kind_name(v),
                o.children.len()
            ));
            return; // Arity is already wrong; recursing would report noise.
        }
        for (child, co) in children(v).iter().zip(&o.children) {
            check_origins(child, co, src, out);
        }
    }

    /// Syntactic children in source order. A map contributes key then value.
    pub fn children(v: &Value) -> Vec<Value> {
        match v {
            Value::List(l) => l.0.clone(),
            Value::Vec(x) => x.0.clone(),
            Value::Map(m) => m.0.iter().flat_map(|(k, v)| [k.clone(), v.clone()]).collect(),
            _ => Vec::new(),
        }
    }

    pub fn kind_name(v: &Value) -> &'static str {
        match v {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::Sym(_) => "symbol",
            Value::Keyword(_) => "keyword",
            Value::List(_) => "list",
            Value::Vec(_) => "vector",
            Value::Map(_) => "map",
            Value::Fn(_) => "fn",
            Value::Cell(_) => "cell",
            Value::Handle(_) => "handle",
            Value::Buffer(_) => "buffer",
        }
    }

    /// The debug rendering ADR-026 calls for. Origins are invisible in ordinary
    /// printed output by construction, so without this the invariants could
    /// only be inspected through a debugger.
    pub fn print_origins(v: &Value, o: &Origins, interner: &Interner, depth: usize) -> String {
        let pad = "  ".repeat(depth);
        let loc = match o.origin {
            SpanOrigin::Source(s) => format!("{}..{}", s.start, s.end),
            SpanOrigin::Generated(s) => format!("generated@{}..{}", s.start, s.end),
            SpanOrigin::Unknown => "unknown".to_string(),
        };
        let mut out = format!(
            "{pad}{} {} {}",
            loc,
            kind_name(v),
            crate::printer::print(v, interner)
        );
        for (child, co) in children(v).iter().zip(&o.children) {
            out.push('\n');
            out.push_str(&print_origins(child, co, interner, depth + 1));
        }
        out
    }

    /// ADR-025: the size is asserted, not assumed.
    pub fn report_sizes() -> std::process::ExitCode {
        const LIMIT: usize = 24;
        let n = std::mem::size_of::<Value>();
        println!("Value: {n} bytes (limit {LIMIT}, ADR-025)");
        println!("Origins: {} bytes", std::mem::size_of::<Origins>());
        if n > LIMIT {
            println!("VIOLATION: Value exceeds {LIMIT} bytes");
            return std::process::ExitCode::FAILURE;
        }
        std::process::ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------------------

/// The reader. Character-driven and producing data, not a grammar-shaped tree —
/// which is why there is no separate tokenizer to fight reader-macro dispatch
/// later (ADR-014).
pub mod reader {
    use crate::error::{LispErr, Span, SpanOrigin};
    use crate::value::{
        Interner, ListObj, LocatedForm, MapObj, Origins, StrObj, Value, VecObj,
    };
    use std::rc::Rc;

    pub fn read_all(src: &str, interner: &mut Interner) -> Result<Vec<LocatedForm>, LispErr> {
        let mut r = Reader { src, pos: 0, interner };
        let mut out = Vec::new();
        loop {
            r.skip_ws();
            if r.at_end() {
                return Ok(out);
            }
            out.push(r.read_form()?);
        }
    }

    struct Reader<'a> {
        src: &'a str,
        pos: usize,
        interner: &'a mut Interner,
    }

    impl<'a> Reader<'a> {
        fn at_end(&self) -> bool {
            self.pos >= self.src.len()
        }

        fn peek(&self) -> Option<u8> {
            self.src.as_bytes().get(self.pos).copied()
        }

        fn skip_ws(&mut self) {
            loop {
                match self.peek() {
                    // Commas are whitespace, as in Clojure.
                    Some(b) if b.is_ascii_whitespace() || b == b',' => {
                        self.pos += 1;
                    }
                    Some(b';') => {
                        while let Some(b) = self.peek() {
                            if b == b'\n' {
                                break;
                            }
                            self.pos += 1;
                        }
                    }
                    _ => return,
                }
            }
        }

        fn err_here(&self, msg: impl Into<String>) -> LispErr {
            LispErr::at(Span::new(self.pos, self.pos), msg)
        }

        fn read_form(&mut self) -> Result<LocatedForm, LispErr> {
            self.skip_ws();
            let start = self.pos;
            let b = match self.peek() {
                Some(b) => b,
                None => return Err(self.err_here("unexpected end of input")),
            };

            match b {
                b'(' => self.read_seq(b')', start, |items| {
                    Value::List(Rc::new(ListObj(items)))
                }),
                b'[' => self.read_seq(b']', start, |items| {
                    Value::Vec(Rc::new(VecObj(items)))
                }),
                b'{' => self.read_map(start),
                b')' | b']' | b'}' => {
                    self.pos += 1;
                    Err(LispErr::at(
                        Span::new(start, self.pos),
                        format!("unmatched `{}`", b as char),
                    ))
                }
                b'"' => self.read_string(start),
                b'\'' => {
                    self.pos += 1;
                    let inner = self.read_form()?;
                    let span = Span::new(start, self.pos);
                    let quote = self.interner.sym("quote");
                    // `'x` reads as `(quote x)`. The synthesized `quote` symbol
                    // has no source text of its own, so it takes the span of
                    // the sugar that produced it rather than claiming a
                    // position it does not occupy.
                    Ok(LocatedForm {
                        root: Value::List(Rc::new(ListObj(vec![quote, inner.root]))),
                        origins: Origins {
                            origin: SpanOrigin::Source(span),
                            children: vec![
                                Origins::leaf(SpanOrigin::Source(Span::new(start, start + 1))),
                                inner.origins,
                            ],
                        },
                    })
                }
                b':' => self.read_keyword(start),
                _ => self.read_atom(start),
            }
        }

        fn read_seq(
            &mut self,
            close: u8,
            start: usize,
            build: impl Fn(Vec<Value>) -> Value,
        ) -> Result<LocatedForm, LispErr> {
            self.pos += 1; // opening delimiter
            let mut items = Vec::new();
            let mut origins = Vec::new();
            loop {
                self.skip_ws();
                match self.peek() {
                    None => {
                        // Report the opener, not the end of the file: in a long
                        // file the unclosed delimiter is the only position that
                        // tells you anything.
                        return Err(LispErr::at(
                            Span::new(start, start + 1),
                            format!("unclosed `{}`", self.src.as_bytes()[start] as char),
                        ));
                    }
                    Some(b) if b == close => {
                        self.pos += 1;
                        let span = Span::new(start, self.pos);
                        return Ok(LocatedForm {
                            root: build(items),
                            origins: Origins {
                                origin: SpanOrigin::Source(span),
                                children: origins,
                            },
                        });
                    }
                    _ => {
                        let f = self.read_form()?;
                        items.push(f.root);
                        origins.push(f.origins);
                    }
                }
            }
        }

        fn read_map(&mut self, start: usize) -> Result<LocatedForm, LispErr> {
            self.pos += 1;
            let mut pairs: Vec<(Value, Value)> = Vec::new();
            let mut origins = Vec::new();
            loop {
                self.skip_ws();
                match self.peek() {
                    None => {
                        return Err(LispErr::at(Span::new(start, start + 1), "unclosed `{`"));
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        let span = Span::new(start, self.pos);
                        return Ok(LocatedForm {
                            root: Value::Map(Rc::new(MapObj(pairs))),
                            origins: Origins {
                                origin: SpanOrigin::Source(span),
                                children: origins,
                            },
                        });
                    }
                    _ => {
                        let k = self.read_form()?;
                        self.skip_ws();
                        // Running out of input is an unclosed brace, not an odd
                        // key count. Both are true of `{:a`, but only one of
                        // them tells you what to type next.
                        match self.peek() {
                            None => {
                                return Err(LispErr::at(
                                    Span::new(start, start + 1),
                                    "unclosed `{`",
                                ))
                            }
                            Some(b'}') => {
                                return Err(LispErr::at(
                                    k.origins.origin.span().unwrap_or(Span::new(start, start + 1)),
                                    "map literal has a key with no value",
                                ))
                            }
                            _ => {}
                        }
                        let v = self.read_form()?;
                        // Q20 owns duplicate keys. Reading them through keeps
                        // the reader honest about what the source said; a later
                        // phase can reject or collapse.
                        pairs.push((k.root, v.root));
                        origins.push(k.origins);
                        origins.push(v.origins);
                    }
                }
            }
        }

        fn read_string(&mut self, start: usize) -> Result<LocatedForm, LispErr> {
            self.pos += 1;
            let mut s = String::new();
            loop {
                let b = match self.peek() {
                    None => {
                        return Err(LispErr::at(
                            Span::new(start, start + 1),
                            "unclosed string literal",
                        ))
                    }
                    Some(b) => b,
                };
                match b {
                    b'"' => {
                        self.pos += 1;
                        return Ok(self.located(
                            Value::Str(Rc::new(StrObj(s))),
                            Span::new(start, self.pos),
                        ));
                    }
                    b'\\' => {
                        // The backslash offset is captured before advancing.
                        // The escaped character may be multi-byte, so the span
                        // cannot be recovered afterwards by subtracting a byte
                        // count — that lands inside a character and panics the
                        // renderer on input the reader is supposed to reject.
                        let esc_start = self.pos;
                        self.pos += 1;
                        let esc = match self.src[self.pos..].chars().next() {
                            None => return Err(self.err_here("unfinished escape sequence")),
                            Some(c) => c,
                        };
                        self.pos += esc.len_utf8();
                        match esc {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            '\\' => s.push('\\'),
                            '"' => s.push('"'),
                            // Reported as a character, not as the first byte of
                            // one: `\€` names the escape the source actually
                            // wrote rather than a mojibake prefix of it.
                            other => {
                                return Err(LispErr::at(
                                    Span::new(esc_start, self.pos),
                                    format!("unknown escape `\\{other}`"),
                                ))
                            }
                        }
                    }
                    _ => {
                        let rest = &self.src[self.pos..];
                        let c = rest.chars().next().unwrap();
                        s.push(c);
                        self.pos += c.len_utf8();
                    }
                }
            }
        }

        fn read_keyword(&mut self, start: usize) -> Result<LocatedForm, LispErr> {
            self.pos += 1; // ':'
            let text_start = self.pos;
            self.scan_token();
            if self.pos == text_start {
                return Err(LispErr::at(Span::new(start, self.pos), "`:` with no name"));
            }
            let name = self.src[text_start..self.pos].to_string();
            let v = self.interner.keyword(&name);
            Ok(self.located(v, Span::new(start, self.pos)))
        }

        fn read_atom(&mut self, start: usize) -> Result<LocatedForm, LispErr> {
            self.scan_token();
            let text = &self.src[start..self.pos];
            if text.is_empty() {
                let c = self.peek().unwrap_or(b' ') as char;
                self.pos += 1;
                return Err(LispErr::at(
                    Span::new(start, self.pos),
                    format!("unexpected `{c}`"),
                ));
            }
            let span = Span::new(start, self.pos);
            let v = match text {
                "nil" => Value::Nil,
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => match parse_number(text) {
                    Some(Ok(v)) => v,
                    Some(Err(why)) => return Err(LispErr::at(span, why)),
                    None => self.interner.sym(text),
                },
            };
            Ok(self.located(v, span))
        }

        fn scan_token(&mut self) {
            while let Some(b) = self.peek() {
                if b.is_ascii_whitespace() || matches!(b, b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b';' | b',') {
                    break;
                }
                self.pos += 1;
            }
        }

        fn located(&self, root: Value, span: Span) -> LocatedForm {
            LocatedForm {
                root,
                origins: Origins::leaf(SpanOrigin::Source(span)),
            }
        }
    }

    /// A token is a number only if the whole token parses. `1abc` is a symbol,
    /// not the integer 1 followed by junk — a partial parse would silently
    /// accept typos.
    ///
    /// `None` means "not numeric-looking, treat as a symbol". `Some(Err)` means
    /// it looked like a number and was not one; that is an error rather than a
    /// symbol, because an integer too large for `i64` reaching the compiler as
    /// a variable name is a silent wrong answer.
    fn parse_number(text: &str) -> Option<Result<Value, String>> {
        let looks_numeric = {
            let mut cs = text.chars();
            match cs.next() {
                Some(c) if c.is_ascii_digit() => true,
                Some('-') | Some('+') => cs.next().is_some_and(|c| c.is_ascii_digit()),
                _ => false,
            }
        };
        if !looks_numeric {
            return None;
        }
        if let Ok(i) = text.parse::<i64>() {
            return Some(Ok(Value::Int(i)));
        }
        // An integer literal too large for i64 is rejected rather than widened
        // to a float, which would lose digits without saying so. Q10 owns
        // overflow at runtime; this is the literal, which is decidable here.
        if !text.contains(['.', 'e', 'E']) {
            return Some(Err(format!("number `{text}` does not fit in a 64-bit integer")));
        }
        if let Ok(f) = text.parse::<f64>() {
            return Some(Ok(Value::Float(f)));
        }
        Some(Err(format!("`{text}` is not a valid number")))
    }
}

// ---------------------------------------------------------------------------

/// The printer. Paired with the reader by the round-trip property (BUILD.md).
pub mod printer {
    use crate::value::{Interner, Value};

    pub fn print(v: &Value, interner: &Interner) -> String {
        let mut out = String::new();
        write(&mut out, v, interner);
        out
    }

    fn write(out: &mut String, v: &Value, interner: &Interner) {
        match v {
            Value::Nil => out.push_str("nil"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Float(f) => out.push_str(&print_float(*f)),
            Value::Str(s) => write_string(out, &s.0),
            Value::Bytes(b) => {
                out.push_str("#bytes[");
                for (i, byte) in b.0.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    out.push_str(&byte.to_string());
                }
                out.push(']');
            }
            Value::Sym(s) => out.push_str(interner.name(s.0)),
            Value::Keyword(k) => {
                out.push(':');
                out.push_str(interner.name(k.0));
            }
            Value::List(l) => write_seq(out, &l.0, '(', ')', interner),
            Value::Vec(v) => write_seq(out, &v.0, '[', ']', interner),
            Value::Map(m) => {
                out.push('{');
                for (i, (k, val)) in m.0.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    write(out, k, interner);
                    out.push(' ');
                    write(out, val, interner);
                }
                out.push('}');
            }
            // These have no reader syntax. They print as opaque rather than as
            // something that would read back as a different value.
            Value::Fn(_) => out.push_str("#<fn>"),
            Value::Cell(c) => out.push_str(&format!("#<cell {}:{}>", c.0, c.1)),
            Value::Handle(h) => out.push_str(&format!("#<handle {}:{}>", h.0, h.1)),
            Value::Buffer(b) => out.push_str(&format!("#<buffer {}:{}>", b.0, b.1)),
        }
    }

    fn write_seq(out: &mut String, items: &[Value], open: char, close: char, interner: &Interner) {
        out.push(open);
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            write(out, item, interner);
        }
        out.push(close);
    }

    fn write_string(out: &mut String, s: &str) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                _ => out.push(c),
            }
        }
        out.push('"');
    }

    /// Floats must print in a form that reads back as a float. Rust's default
    /// renders 1.0 as "1", which would read back as an integer and quietly
    /// break the round-trip property.
    fn print_float(f: f64) -> String {
        if f.is_nan() {
            return "##NaN".to_string();
        }
        if f.is_infinite() {
            return if f > 0.0 { "##Inf".into() } else { "##-Inf".into() };
        }
        let s = f.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.0")
        }
    }
}
