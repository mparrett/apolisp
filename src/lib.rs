//! apolisp — a small Lisp in the Clojure dialect with its own VM.
//!
//! One library file with inline `mod` blocks (ADR-015, ADR-031). The module
//! paths are the ones chosen up front, so extraction into files later is a move
//! rather than a redesign. The seams are for subtraction: each `mod` below
//! should be cuttable or liftable, not merely a home for code.
//!
//! `main.rs` is the process driver and holds no language behaviour, so the
//! reader, printer, and values can be tested as values rather than through a
//! subprocess (ADR-031).
//!
//! Milestone 1 (BUILD.md): reader + printer + forms with span origins.
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
            Span {
                start: start as u32,
                end: end as u32,
            }
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
            LispErr {
                msg: msg.into(),
                origin: SpanOrigin::Source(span),
            }
        }

        /// For phases downstream of the reader, whose input may be generated
        /// rather than read. A compiler error on macro output has no file
        /// position, and inventing one is worse than saying so (ADR-026).
        pub fn at_origin(origin: SpanOrigin, msg: impl Into<String>) -> LispErr {
            LispErr {
                msg: msg.into(),
                origin,
            }
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

    /// An index into the VM's native function table (ADR-038).
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct NativeId(pub u32);

    /// ADR-038. A native function is a kind of closure, so it is called through
    /// the ordinary `Call` and `Value` needs no new variant — ADR-025 stands.
    ///
    /// `proto` is a plain `u32` rather than `bytecode::ProtoIdx` because
    /// `bytecode` depends on this module and not the other way round. Captures
    /// are copied in at creation and never referenced (ADR-002).
    #[derive(Debug)]
    pub enum Closure {
        Fn { proto: u32, captures: Rc<[Value]> },
        Native(NativeId),
    }

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
    #[derive(Debug)]
    pub struct Interner {
        names: Vec<String>,
        index: HashMap<String, u32>,
    }

    impl Interner {
        pub fn new() -> Interner {
            Interner {
                names: Vec::new(),
                index: HashMap::new(),
            }
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
            Origins {
                origin,
                children: Vec::new(),
            }
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
            Value::Map(m) => {
                m.0.iter()
                    .flat_map(|(k, v)| [k.clone(), v.clone()])
                    .collect()
            }
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

    /// ADR-025: the size is asserted, not assumed. The limit and the
    /// measurement live here, next to the type; printing them is the driver's
    /// job, and a test can now assert the number rather than parse a report.
    pub const VALUE_SIZE_LIMIT: usize = 24;

    pub fn value_size() -> usize {
        std::mem::size_of::<Value>()
    }

    pub fn origins_size() -> usize {
        std::mem::size_of::<Origins>()
    }
}

// ---------------------------------------------------------------------------

/// The reader. Character-driven and producing data, not a grammar-shaped tree —
/// which is why there is no separate tokenizer to fight reader-macro dispatch
/// later (ADR-014).
pub mod reader {
    use crate::error::{LispErr, Span, SpanOrigin};
    use crate::value::{Interner, ListObj, LocatedForm, MapObj, Origins, StrObj, Value, VecObj};
    use std::rc::Rc;

    /// ADR-036. Every phase in front of the VM recurses on the host stack — the
    /// reader, the origin walkers, `Rc`'s drop glue over a nested list, the
    /// resolver, the lowering — and each of them recurses at most once per level
    /// of form nesting. So bounding depth here bounds all of them, and this is
    /// the only place that checks.
    ///
    /// Sized for the smallest stack this runs on, which is a 2 MB cargo test
    /// thread rather than the 8 MB main thread.
    pub const MAX_NESTING: usize = 256;

    pub fn read_all(src: &str, interner: &mut Interner) -> Result<Vec<LocatedForm>, LispErr> {
        let mut r = Reader {
            src,
            pos: 0,
            depth: 0,
            interner,
        };
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
        depth: usize,
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

        /// The depth bound (ADR-036) lives here rather than in each recursive
        /// case, because this is the single point every one of them goes
        /// through. Without it a deeply nested file overflows the host stack and
        /// the process is killed, which is a stack overflow presenting as a
        /// crash rather than as a diagnostic.
        fn read_form(&mut self) -> Result<LocatedForm, LispErr> {
            if self.depth >= MAX_NESTING {
                self.skip_ws();
                return Err(self.err_here(format!("nested more than {MAX_NESTING} deep (ADR-036)")));
            }
            self.depth += 1;
            let form = self.read_form_inner();
            self.depth -= 1;
            form
        }

        fn read_form_inner(&mut self) -> Result<LocatedForm, LispErr> {
            self.skip_ws();
            let start = self.pos;
            let b = match self.peek() {
                Some(b) => b,
                None => return Err(self.err_here("unexpected end of input")),
            };

            match b {
                b'(' => self.read_seq(b')', start, |items| Value::List(Rc::new(ListObj(items)))),
                b'[' => self.read_seq(b']', start, |items| Value::Vec(Rc::new(VecObj(items)))),
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
                                    k.origins
                                        .origin
                                        .span()
                                        .unwrap_or(Span::new(start, start + 1)),
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
                        return Ok(self
                            .located(Value::Str(Rc::new(StrObj(s))), Span::new(start, self.pos)));
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
                // Clojure's spellings, and exactly what the printer emits. A
                // printer that emits tokens its own reader cannot read is a
                // round-trip hole: `##Inf` read back as a symbol prints as
                // `##Inf` again, so string-level comparison never notices the
                // type change (ADR-032).
                "##Inf" => Value::Float(f64::INFINITY),
                "##-Inf" => Value::Float(f64::NEG_INFINITY),
                "##NaN" => Value::Float(f64::NAN),
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
                if b.is_ascii_whitespace()
                    || matches!(
                        b,
                        b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b';' | b','
                    )
                {
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
            return Some(Err(format!(
                "number `{text}` does not fit in a 64-bit integer"
            )));
        }
        if let Ok(f) = text.parse::<f64>() {
            // A finite-looking literal that overflows the range is rejected for
            // the same reason an oversized integer is: `1e400` becoming
            // infinity is a silent wrong answer, and the digits the source
            // wrote are gone. `##Inf` says it on purpose (ADR-032).
            //
            // Underflow is deliberately not symmetric — `1e-400` reads as zero,
            // as it does everywhere else. Losing precision near zero is the
            // ordinary condition of floating point; losing the value entirely
            // is not.
            if f.is_infinite() {
                return Some(Err(format!(
                    "number `{text}` overflows to infinity; write `##Inf` to mean it"
                )));
            }
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

    /// What `println` emits: a string is its own characters, not a readable
    /// literal. Clojure draws the same line between `pr` and `print`, and the
    /// round-trip property is about `print` — this one deliberately does not
    /// read back.
    ///
    /// Only the top level is affected. A string *inside* a collection still
    /// prints readably, because the alternative renders `["a b" "c"]`
    /// indistinguishable from `["a" "b" "c"]`.
    pub fn display(v: &Value, interner: &Interner) -> String {
        match v {
            Value::Str(s) => s.0.clone(),
            _ => print(v, interner),
        }
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
            return if f > 0.0 {
                "##Inf".into()
            } else {
                "##-Inf".into()
            };
        }
        let s = f.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.0")
        }
    }
}

// ---------------------------------------------------------------------------

/// The instruction set, the compiled function, and the disassembler (ADR-034).
///
/// Instructions are a typed enum with `u32` operands rather than packed words.
/// E-5 corrected ADR-006's claim that monotonic slots avoid a wider encoding —
/// no reuse raises the maximum live slot index, so a packed format's operand
/// fields are exactly what comes under pressure. Unpacked, there is no width
/// left to run out of.
pub mod bytecode {
    use crate::error::{line_col, SpanOrigin};
    use crate::printer;
    use crate::value::{Interner, SymId, Value};

    pub type Slot = u32;
    pub type ConstIdx = u32;
    pub type Pc = u32;
    pub type ProtoIdx = u32;
    pub type CaptureIdx = u32;

    /// A call's callee sits at `base` and its arguments at `base+1 ..= base+argc`
    /// — where left-to-right evaluation into monotonically allocated slots puts
    /// them anyway (ADR-033). The callee's frame receives the arguments in its
    /// own slots `0..argc`, so parameters are the first slots of a frame by
    /// construction.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Instr {
        Const {
            dst: Slot,
            k: ConstIdx,
        },
        Move {
            dst: Slot,
            src: Slot,
        },
        /// The operand is the interned name, not a `CellId`: forward references
        /// then need no fixup, and the disassembly does not depend on the order
        /// globals were defined in (ADR-034). The VM still owns the cell
        /// (ADR-027).
        GetGlobal {
            dst: Slot,
            name: SymId,
        },
        SetGlobal {
            name: SymId,
            src: Slot,
        },
        GetCapture {
            dst: Slot,
            idx: CaptureIdx,
        },
        /// ADR-002: self-recursion resolves through the running closure's own
        /// identity, never through a capture.
        GetSelf {
            dst: Slot,
        },
        SetCell {
            cell: Slot,
            src: Slot,
        },
        Closure {
            dst: Slot,
            proto: ProtoIdx,
        },
        Call {
            dst: Slot,
            base: Slot,
            argc: u32,
        },
        TailCall {
            base: Slot,
            argc: u32,
        },
        Return {
            src: Slot,
        },
        Jump {
            target: Pc,
        },
        /// Only `nil` and `false` are falsy (`TRAPS.md`).
        JumpUnless {
            cond: Slot,
            target: Pc,
        },
        Throw {
            src: Slot,
        },
        PushHandler {
            catch: Pc,
            err: Slot,
        },
        PushFinally {
            target: Pc,
        },
        PopHandler,
        EndFinally,
    }

    /// ADR-034 asserts this the way ADR-025 asserts `Value`'s: the number is
    /// measured, not assumed. The point of not packing was to stop paying
    /// attention to operand widths, and the assertion is what keeps that from
    /// becoming an excuse for an instruction that carries a `String`.
    pub const INSTR_SIZE_LIMIT: usize = 16;

    pub fn instr_size() -> usize {
        std::mem::size_of::<Instr>()
    }

    /// Where a capture's value comes from, read in the **enclosing** frame at
    /// the moment `Closure` runs. Copied, not referenced — that is the whole of
    /// ADR-002.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum CaptureSrc {
        Local(Slot),
        Capture(CaptureIdx),
        SelfFn,
    }

    /// One compiled function. A *prototype* rather than a function because it
    /// holds no captured values: a closure is this plus the values captured at
    /// the moment it was created.
    #[derive(Debug)]
    pub struct Proto {
        pub name: Option<SymId>,
        /// Named parameters, including the rest parameter when `variadic`.
        pub params: u32,
        pub variadic: bool,
        pub slots: u32,
        pub code: Vec<Instr>,
        /// ADR-023 point 2: `lines[i]` is the origin of the instruction at `i`.
        /// Kept parallel to `code` by construction — every instruction is
        /// emitted through one function that pushes to both.
        pub lines: Vec<SpanOrigin>,
        pub consts: Vec<Value>,
        pub captures: Vec<CaptureSrc>,
    }

    /// What compiling one file produces: `protos[0]` is the top level, and
    /// every nested `fn` is an index into this vector, reserved in source order
    /// so the disassembly is stable (ADR-034).
    #[derive(Debug)]
    pub struct Chunk {
        pub protos: Vec<Proto>,
    }

    pub fn disassemble(chunk: &Chunk, interner: &Interner, src: &str) -> String {
        let mut out = String::new();
        for (i, p) in chunk.protos.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            disasm_proto(&mut out, i, p, interner, src);
        }
        out
    }

    fn disasm_proto(out: &mut String, idx: usize, p: &Proto, interner: &Interner, src: &str) {
        let name = match (p.name, idx) {
            (Some(s), _) => interner.name(s.0),
            (None, 0) => "<top>",
            (None, _) => "<fn>",
        };
        let params = if p.variadic {
            format!("{} &rest", p.params - 1)
        } else {
            p.params.to_string()
        };
        out.push_str(&format!(
            "proto {idx}  {name}  params {params}  slots {}\n",
            p.slots
        ));
        if !p.consts.is_empty() {
            out.push_str("  constants\n");
            for (k, v) in p.consts.iter().enumerate() {
                out.push_str(&format!("    k{k}  {}\n", printer::print(v, interner)));
            }
        }
        if !p.captures.is_empty() {
            out.push_str("  captures\n");
            for (c, s) in p.captures.iter().enumerate() {
                let from = match *s {
                    CaptureSrc::Local(slot) => format!("local r{slot}"),
                    CaptureSrc::Capture(i) => format!("capture c{i}"),
                    CaptureSrc::SelfFn => "self".to_string(),
                };
                out.push_str(&format!("    c{c}  {from}\n"));
            }
        }
        out.push_str("  code\n");
        for (pc, ins) in p.code.iter().enumerate() {
            let (mnemonic, operands) = render(ins, interner);
            out.push_str(&format!(
                "    {pc:04}  {mnemonic:<11} {operands:<22} {}\n",
                position(p.lines[pc], src)
            ));
        }
    }

    /// The position half of ADR-023 point 2. `../reg-lisp`'s completeness test
    /// found *every* function's return attributed to line 0 — a hole a corpus
    /// does not surface, because the output still looks right — so the position
    /// prints on every instruction rather than where it seems interesting.
    fn position(o: SpanOrigin, src: &str) -> String {
        match o {
            SpanOrigin::Source(s) => {
                let (line, col) = line_col(src, s.start as usize);
                format!("{line}:{col}")
            }
            SpanOrigin::Generated(s) => {
                let (line, col) = line_col(src, s.start as usize);
                format!("generated {line}:{col}")
            }
            SpanOrigin::Unknown => "?".to_string(),
        }
    }

    fn render(i: &Instr, interner: &Interner) -> (&'static str, String) {
        match *i {
            Instr::Const { dst, k } => ("CONST", format!("r{dst} <- k{k}")),
            Instr::Move { dst, src } => ("MOVE", format!("r{dst} <- r{src}")),
            Instr::GetGlobal { dst, name } => {
                ("GETGLOBAL", format!("r{dst} <- {}", interner.name(name.0)))
            }
            Instr::SetGlobal { name, src } => {
                ("SETGLOBAL", format!("{} <- r{src}", interner.name(name.0)))
            }
            Instr::GetCapture { dst, idx } => ("GETCAP", format!("r{dst} <- c{idx}")),
            Instr::GetSelf { dst } => ("GETSELF", format!("r{dst} <- self")),
            Instr::SetCell { cell, src } => ("SETCELL", format!("[r{cell}] <- r{src}")),
            Instr::Closure { dst, proto } => ("CLOSURE", format!("r{dst} <- proto {proto}")),
            Instr::Call { dst, base, argc } => ("CALL", format!("r{dst} <- r{base}({argc})")),
            Instr::TailCall { base, argc } => ("TAILCALL", format!("r{base}({argc})")),
            Instr::Return { src } => ("RETURN", format!("r{src}")),
            Instr::Jump { target } => ("JUMP", format!("{target:04}")),
            Instr::JumpUnless { cond, target } => ("JUMPUNLESS", format!("r{cond}, {target:04}")),
            Instr::Throw { src } => ("THROW", format!("r{src}")),
            Instr::PushHandler { catch, err } => {
                ("PUSHHANDLER", format!("catch {catch:04}, err r{err}"))
            }
            Instr::PushFinally { target } => ("PUSHFINALLY", format!("{target:04}")),
            Instr::PopHandler => ("POPHANDLER", String::new()),
            Instr::EndFinally => ("ENDFINALLY", String::new()),
        }
    }
}

// ---------------------------------------------------------------------------

/// The core AST and the slot compiler (ADR-006, ADR-007, ADR-034).
///
/// Two passes. The resolver turns forms into `Core`, deciding what every symbol
/// *is* — a local, a capture, the running function itself, or a global — and
/// nothing about layout. The lowering assigns slots monotonically and emits
/// instructions. Splitting them is what keeps slot allocation in one place:
/// bindings and temporaries come out of the same counter, which is the whole of
/// "no liveness analysis, no reuse".
pub mod compile {
    use crate::bytecode::{
        CaptureIdx, CaptureSrc, Chunk, ConstIdx, Instr, Pc, Proto, ProtoIdx, Slot,
    };
    use crate::error::{LispErr, SpanOrigin};
    use crate::value::{kind_name, Interner, LocatedForm, Origins, SymId, Value};

    pub type LocalId = u32;

    // --- the core AST -------------------------------------------------------

    /// The closed core (ADR-007, amended by ADR-027, spelled by ADR-034). Read
    /// this enum and you have read the language.
    ///
    /// Thirteen forms, more variants: `literal` absorbs `quote`, and `local`
    /// arrives as one of three resolutions — a slot in this frame, a capture, or
    /// the running closure itself. Which one it is is the resolver's whole job.
    #[derive(Debug)]
    pub enum Core {
        Literal(Value),
        Local(LocalId),
        Capture(CaptureIdx),
        SelfFn,
        Global(SymId),
        If(Box<Expr>, Box<Expr>, Box<Expr>),
        Do(Vec<Expr>),
        Let(Vec<(LocalId, Expr)>, Vec<Expr>),
        Fn(Box<FnDef>),
        Call(Box<Expr>, Vec<Expr>),
        SetCell(Box<Expr>, Box<Expr>),
        SetGlobal(SymId, Box<Expr>),
        Throw(Box<Expr>),
        Try(Box<TryForm>),
    }

    /// A core node and where it came from. The origin travels on every node
    /// because `lines[i]` needs one per *instruction* (ADR-023 point 2), and an
    /// instruction is emitted from a node.
    #[derive(Debug)]
    pub struct Expr {
        pub core: Core,
        pub origin: SpanOrigin,
    }

    #[derive(Debug)]
    pub struct FnDef {
        pub name: Option<SymId>,
        /// Named parameters, including the rest parameter when `variadic`.
        pub params: u32,
        pub variadic: bool,
        /// How many `LocalId`s this function hands out. Parameters are 0..params.
        pub locals: u32,
        pub captures: Vec<CaptureSpec>,
        pub body: Vec<Expr>,
        pub origin: SpanOrigin,
    }

    /// A capture named in the terms the *resolver* has — local ids, not slots.
    /// Slots do not exist until lowering, and the translation happens in the
    /// enclosing function, which is where the value is read from.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum CaptureSpec {
        Local(LocalId),
        Capture(CaptureIdx),
        SelfFn,
    }

    #[derive(Debug)]
    pub struct TryForm {
        pub body: Vec<Expr>,
        pub catch: Option<(LocalId, Vec<Expr>)>,
        pub finally: Option<Vec<Expr>>,
    }

    // --- walking forms ------------------------------------------------------

    /// A value and the origins travelling beside it (ADR-026). This pair is the
    /// unit the resolver walks; splitting them is exactly how spans get lost.
    #[derive(Clone, Copy)]
    struct Form<'a> {
        v: &'a Value,
        o: &'a Origins,
    }

    impl<'a> Form<'a> {
        /// Syntactic children, in source order, paired with their origins. A map
        /// contributes key then value, which is the order ADR-033 fixes for
        /// evaluation too.
        fn items(&self) -> Vec<Form<'a>> {
            let zip = |vs: Vec<&'a Value>| -> Vec<Form<'a>> {
                vs.into_iter()
                    .zip(&self.o.children)
                    .map(|(v, o)| Form { v, o })
                    .collect()
            };
            match self.v {
                Value::List(l) => zip(l.0.iter().collect()),
                Value::Vec(x) => zip(x.0.iter().collect()),
                Value::Map(m) => zip(m.0.iter().flat_map(|(k, v)| [k, v]).collect()),
                _ => Vec::new(),
            }
        }

        fn sym(&self) -> Option<SymId> {
            match self.v {
                Value::Sym(s) => Some(*s),
                _ => None,
            }
        }

        /// The head symbol of a non-empty list, which is what makes a form a
        /// special form or a call.
        fn head(&self) -> Option<SymId> {
            match self.v {
                Value::List(l) => match l.0.first() {
                    Some(Value::Sym(s)) => Some(*s),
                    _ => None,
                },
                _ => None,
            }
        }

        fn err(&self, msg: impl Into<String>) -> LispErr {
            LispErr::at_origin(self.o.origin, msg)
        }
    }

    // --- the resolver -------------------------------------------------------

    /// The core-form names, interned once. A head symbol is compared by id
    /// after this, which is the only comparison symbols are supposed to need.
    struct Specials {
        if_: SymId,
        do_: SymId,
        let_: SymId,
        fn_: SymId,
        quote: SymId,
        set_cell: SymId,
        set_global: SymId,
        throw: SymId,
        try_: SymId,
        catch: SymId,
        finally: SymId,
        rest: SymId,
        vector: SymId,
        hash_map: SymId,
    }

    impl Specials {
        fn new(i: &mut Interner) -> Specials {
            let mut s = |n: &str| SymId(i.intern(n));
            Specials {
                if_: s("if"),
                do_: s("do"),
                let_: s("let"),
                fn_: s("fn"),
                quote: s("quote"),
                set_cell: s("set-cell!"),
                set_global: s("set-global!"),
                throw: s("throw"),
                try_: s("try"),
                catch: s("catch"),
                finally: s("finally"),
                rest: s("&"),
                vector: s("vector"),
                hash_map: s("hash-map"),
            }
        }
    }

    struct FnScope {
        name: Option<SymId>,
        locals: u32,
        scopes: Vec<Vec<(SymId, LocalId)>>,
        captures: Vec<CaptureSpec>,
    }

    impl FnScope {
        fn new(name: Option<SymId>) -> FnScope {
            FnScope {
                name,
                locals: 0,
                scopes: vec![Vec::new()],
                captures: Vec::new(),
            }
        }
    }

    struct Resolver<'a> {
        interner: &'a mut Interner,
        sp: Specials,
        fns: Vec<FnScope>,
    }

    /// Forms to core AST. Every symbol comes out resolved and every core form
    /// comes out shape-checked; nothing about slots has happened yet.
    pub fn resolve(forms: &[LocatedForm], interner: &mut Interner) -> Result<FnDef, LispErr> {
        let sp = Specials::new(interner);
        let mut r = Resolver {
            interner,
            sp,
            fns: vec![FnScope::new(None)],
        };
        let mut body = Vec::new();
        for f in forms {
            body.push(r.expr(Form {
                v: &f.root,
                o: &f.origins,
            })?);
        }
        let scope = r.fns.pop().expect("top-level scope");
        Ok(FnDef {
            name: None,
            params: 0,
            variadic: false,
            locals: scope.locals,
            captures: scope.captures,
            body,
            origin: forms
                .first()
                .map(|f| f.origins.origin)
                .unwrap_or(SpanOrigin::Unknown),
        })
    }

    impl Resolver<'_> {
        fn expr(&mut self, f: Form) -> Result<Expr, LispErr> {
            let origin = f.o.origin;
            let core = match f.v {
                // `()` evaluates to itself, as in Clojure. Every other list is a
                // special form or a call.
                Value::List(l) if l.0.is_empty() => Core::Literal(f.v.clone()),
                Value::List(_) => self.list(f)?,
                // Collection literals lower to calls (ADR-035), which is what
                // keeps the core closed at 13 forms while ADR-033's
                // left-to-right element order still falls out of ordinary
                // argument evaluation. Resolved as globals directly rather than
                // through scope: `[x]` is a vector literal even where a local
                // named `vector` is in scope.
                Value::Vec(_) => self.literal_call(f, self.sp.vector, origin)?,
                Value::Map(_) => self.literal_call(f, self.sp.hash_map, origin)?,
                Value::Sym(s) => self.symbol(*s),
                // ADR-023's cost paragraph: the compiler's input is not closed,
                // because a macro can put anything into code position. This is
                // the decent error it asks for.
                Value::Fn(_) | Value::Cell(_) | Value::Handle(_) | Value::Buffer(_) => {
                    return Err(f.err(format!("cannot compile a {}", kind_name(f.v))))
                }
                _ => Core::Literal(f.v.clone()),
            };
            Ok(Expr { core, origin })
        }

        fn literal_call(
            &mut self,
            f: Form,
            ctor: SymId,
            origin: SpanOrigin,
        ) -> Result<Core, LispErr> {
            let callee = Expr {
                core: Core::Global(ctor),
                origin,
            };
            Ok(Core::Call(Box::new(callee), self.exprs(&f.items())?))
        }

        fn exprs(&mut self, items: &[Form]) -> Result<Vec<Expr>, LispErr> {
            items.iter().map(|f| self.expr(*f)).collect()
        }

        fn symbol(&mut self, s: SymId) -> Core {
            let top = self.fns.len() - 1;
            self.lookup(top, s).unwrap_or(Core::Global(s))
        }

        /// A symbol resolves against the innermost function first, then outward,
        /// adding a capture at every level it crosses. A name that reaches no
        /// binding is a global, and stays unresolved until run time — which is
        /// what makes forward references free (ADR-034).
        fn lookup(&mut self, level: usize, name: SymId) -> Option<Core> {
            for scope in self.fns[level].scopes.iter().rev() {
                if let Some((_, id)) = scope.iter().rev().find(|(n, _)| *n == name) {
                    return Some(Core::Local(*id));
                }
            }
            if self.fns[level].name == Some(name) {
                return Some(Core::SelfFn);
            }
            if level == 0 {
                return None;
            }
            let spec = match self.lookup(level - 1, name)? {
                Core::Local(id) => CaptureSpec::Local(id),
                Core::Capture(i) => CaptureSpec::Capture(i),
                Core::SelfFn => CaptureSpec::SelfFn,
                _ => return None,
            };
            Some(Core::Capture(self.add_capture(level, spec)))
        }

        fn add_capture(&mut self, level: usize, spec: CaptureSpec) -> CaptureIdx {
            let caps = &mut self.fns[level].captures;
            if let Some(i) = caps.iter().position(|c| *c == spec) {
                return i as CaptureIdx;
            }
            caps.push(spec);
            (caps.len() - 1) as CaptureIdx
        }

        fn declare(&mut self, name: SymId) -> LocalId {
            let f = self.fns.last_mut().expect("a function scope");
            let id = f.locals;
            f.locals += 1;
            f.scopes.last_mut().expect("a block scope").push((name, id));
            id
        }

        fn push_scope(&mut self) {
            self.fns
                .last_mut()
                .expect("a function scope")
                .scopes
                .push(Vec::new());
        }

        fn pop_scope(&mut self) {
            self.fns
                .last_mut()
                .expect("a function scope")
                .scopes
                .pop()
                .expect("a block scope");
        }

        /// A list with a symbol head is a special form if the head names one.
        /// Checked before scope, so a core form cannot be shadowed by a local —
        /// the core is closed (ADR-007) and a language whose `if` depends on
        /// what is in scope has no closed core.
        fn list(&mut self, f: Form) -> Result<Core, LispErr> {
            let items = f.items();
            if let Some(h) = f.head() {
                let sp = &self.sp;
                if h == sp.if_ {
                    return self.if_form(f, &items);
                } else if h == sp.do_ {
                    return Ok(Core::Do(self.exprs(&items[1..])?));
                } else if h == sp.let_ {
                    return self.let_form(f, &items);
                } else if h == sp.fn_ {
                    return self.fn_form(f, &items);
                } else if h == sp.quote {
                    if items.len() != 2 {
                        return Err(f.err("`quote` takes exactly one form"));
                    }
                    return Ok(Core::Literal(items[1].v.clone()));
                } else if h == sp.set_cell {
                    if items.len() != 3 {
                        return Err(f.err("`set-cell!` takes a cell and a value"));
                    }
                    let cell = self.expr(items[1])?;
                    let val = self.expr(items[2])?;
                    return Ok(Core::SetCell(Box::new(cell), Box::new(val)));
                } else if h == sp.set_global {
                    return self.set_global_form(f, &items);
                } else if h == sp.throw {
                    if items.len() != 2 {
                        return Err(f.err("`throw` takes exactly one value"));
                    }
                    return Ok(Core::Throw(Box::new(self.expr(items[1])?)));
                } else if h == sp.try_ {
                    return self.try_form(&items);
                } else if h == sp.catch || h == sp.finally {
                    let name = self.interner.name(h.0).to_string();
                    return Err(f.err(format!("`{name}` is only valid inside `try`")));
                }
            }
            let callee = self.expr(items[0])?;
            Ok(Core::Call(Box::new(callee), self.exprs(&items[1..])?))
        }

        fn if_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            if items.len() < 3 || items.len() > 4 {
                return Err(f.err("`if` takes a test, a then, and an optional else"));
            }
            let test = self.expr(items[1])?;
            let then = self.expr(items[2])?;
            let other = match items.get(3) {
                Some(e) => self.expr(*e)?,
                // A missing else is `nil`, and it takes the `if`'s own position:
                // there is no source text to point at, and `Unknown` here would
                // lose the one thing a backtrace could say.
                None => Expr {
                    core: Core::Literal(Value::Nil),
                    origin: f.o.origin,
                },
            };
            Ok(Core::If(Box::new(test), Box::new(then), Box::new(other)))
        }

        fn let_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            if items.len() < 2 {
                return Err(f.err("`let` takes a binding vector and a body"));
            }
            if !matches!(items[1].v, Value::Vec(_)) {
                return Err(items[1].err(format!(
                    "`let` takes a binding vector, not a {}",
                    kind_name(items[1].v)
                )));
            }
            let pairs = items[1].items();
            if !pairs.len().is_multiple_of(2) {
                return Err(items[1].err("binding vector has a name with no value"));
            }
            self.push_scope();
            let mut binds = Vec::new();
            for pair in pairs.chunks(2) {
                let name = pair[0].sym().ok_or_else(|| {
                    pair[0].err(format!(
                        "`let` binds symbols, not a {}",
                        kind_name(pair[0].v)
                    ))
                })?;
                // Sequential, left to right (ADR-033): the initializer is
                // resolved before its own name is in scope, so `(let [x x] ...)`
                // takes the outer `x`.
                let init = self.expr(pair[1])?;
                binds.push((self.declare(name), init));
            }
            let body = self.exprs(&items[2..])?;
            self.pop_scope();
            Ok(Core::Let(binds, body))
        }

        fn fn_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            let mut at = 1;
            let name = match items.get(at).and_then(|i| i.sym()) {
                Some(s) => {
                    at += 1;
                    Some(s)
                }
                None => None,
            };
            let params = match items.get(at) {
                Some(p) if matches!(p.v, Value::Vec(_)) => *p,
                Some(p) => {
                    return Err(p.err(format!(
                        "`fn` takes a parameter vector, not a {}",
                        kind_name(p.v)
                    )))
                }
                None => return Err(f.err("`fn` takes a parameter vector and a body")),
            };
            self.fns.push(FnScope::new(name));
            // Popped whether or not the body resolved, so an error does not
            // leave the resolver holding a scope that no longer exists.
            let built = self.fn_inner(params, &items[at + 1..]);
            let scope = self.fns.pop().expect("the function scope just pushed");
            let (count, variadic, body) = built?;
            Ok(Core::Fn(Box::new(FnDef {
                name,
                params: count,
                variadic,
                locals: scope.locals,
                captures: scope.captures,
                body,
                origin: f.o.origin,
            })))
        }

        fn fn_inner(
            &mut self,
            params: Form,
            body: &[Form],
        ) -> Result<(u32, bool, Vec<Expr>), LispErr> {
            let (count, variadic) = self.params(params)?;
            Ok((count, variadic, self.exprs(body)?))
        }

        /// `[a b & rest]`. One parameter list, optionally ending in `&` and one
        /// rest parameter (ADR-033 rule 3). Parameters are declared first, so
        /// they hold local ids `0..params` and therefore slots `0..params` —
        /// which is where a call leaves the arguments (ADR-034).
        fn params(&mut self, f: Form) -> Result<(u32, bool), LispErr> {
            let items = f.items();
            let mut count = 0;
            let mut i = 0;
            while i < items.len() {
                let name = items[i].sym().ok_or_else(|| {
                    items[i].err(format!(
                        "parameters are symbols, not a {}",
                        kind_name(items[i].v)
                    ))
                })?;
                if name == self.sp.rest {
                    let rest = items.get(i + 1).and_then(|p| p.sym());
                    match rest {
                        Some(r) if r != self.sp.rest && i + 2 == items.len() => {
                            self.declare(r);
                            return Ok((count + 1, true));
                        }
                        _ => {
                            return Err(
                                items[i].err("`&` must be followed by exactly one rest parameter")
                            )
                        }
                    }
                }
                self.declare(name);
                count += 1;
                i += 1;
            }
            Ok((count, false))
        }

        fn set_global_form(&mut self, f: Form, items: &[Form]) -> Result<Core, LispErr> {
            if items.len() != 3 {
                return Err(f.err("`set-global!` takes a name and a value"));
            }
            let name = items[1].sym().ok_or_else(|| {
                items[1].err(format!(
                    "`set-global!` binds a symbol, not a {}",
                    kind_name(items[1].v)
                ))
            })?;
            Ok(Core::SetGlobal(name, Box::new(self.expr(items[2])?)))
        }

        fn try_form(&mut self, items: &[Form]) -> Result<Core, LispErr> {
            let mut body = Vec::new();
            let mut catch = None;
            let mut finally = None;
            for it in &items[1..] {
                let head = it.head();
                if head == Some(self.sp.catch) {
                    if catch.is_some() {
                        return Err(it.err("`try` takes at most one `catch` (Q23)"));
                    }
                    if finally.is_some() {
                        return Err(it.err("`finally` must be the last clause"));
                    }
                    let ci = it.items();
                    let bound = ci
                        .get(1)
                        .and_then(|c| c.sym())
                        .ok_or_else(|| it.err("`catch` binds the thrown value to one symbol"))?;
                    self.push_scope();
                    let id = self.declare(bound);
                    let handler = self.exprs(&ci[2..]);
                    self.pop_scope();
                    catch = Some((id, handler?));
                } else if head == Some(self.sp.finally) {
                    if finally.is_some() {
                        return Err(it.err("`try` takes at most one `finally`"));
                    }
                    finally = Some(self.exprs(&it.items()[1..])?);
                } else if catch.is_some() || finally.is_some() {
                    return Err(
                        it.err("everything after a `catch` or `finally` clause must be one too")
                    );
                } else {
                    body.push(self.expr(*it)?);
                }
            }
            Ok(Core::Try(Box::new(TryForm {
                body,
                catch,
                finally,
            })))
        }
    }

    // --- lowering -----------------------------------------------------------

    pub fn compile(forms: &[LocatedForm], interner: &mut Interner) -> Result<Chunk, LispErr> {
        Ok(lower(&resolve(forms, interner)?))
    }

    /// Core AST to a `Chunk`. Slots come out of one monotonic counter per
    /// function, shared by bindings and temporaries and never reused (ADR-006).
    /// E-5 is what makes that affordable here: with unpacked operands a high
    /// slot index costs nothing at all.
    pub fn lower(top: &FnDef) -> Chunk {
        let mut lo = Lower { protos: Vec::new() };
        lo.proto(top, Vec::new());
        Chunk {
            protos: lo
                .protos
                .into_iter()
                .map(|p| p.expect("every reserved proto is filled in"))
                .collect(),
        }
    }

    struct Lower {
        /// An index is reserved before its body is lowered, so a proto's number
        /// is its source order rather than the order compilation finished.
        protos: Vec<Option<Proto>>,
    }

    impl Lower {
        fn proto(&mut self, def: &FnDef, captures: Vec<CaptureSrc>) -> ProtoIdx {
            let idx = self.protos.len();
            self.protos.push(None);
            let mut f = FnLower::new(def);
            let dst = f.alloc(1);
            f.body(self, &def.body, dst, true, def.origin);
            // The return belongs to the expression whose value it returns, not
            // to the `fn` form. For the top-level proto those differ by the
            // whole file: `def.origin` is the *first* form, so a `RETURN` was
            // reporting a position with no relationship to it (ADR-023 point 2
            // exists so a frame's position means something).
            //
            // Unreachable after a tail call, and emitted anyway. One dead
            // instruction is cheaper than a special case in the single place a
            // function's exit is guaranteed to exist.
            let exit = def.body.last().map(|e| e.origin).unwrap_or(def.origin);
            f.emit(Instr::Return { src: dst }, exit);
            self.protos[idx] = Some(Proto {
                name: def.name,
                params: def.params,
                variadic: def.variadic,
                slots: f.next,
                code: f.code,
                lines: f.lines,
                consts: f.consts,
                captures,
            });
            idx as ProtoIdx
        }
    }

    struct FnLower {
        code: Vec<Instr>,
        lines: Vec<SpanOrigin>,
        consts: Vec<Value>,
        /// `LocalId` to slot, filled in at the binding, read at every use.
        slots: Vec<Slot>,
        next: Slot,
        /// Handler regions open in this function. A call inside one is never a
        /// tail call — ADR-028 rule 2 says the frame is still needed, and that
        /// reason covers `catch` exactly as it covers `finally`: the handler
        /// record names this frame, and a reused frame is a different one.
        regions: u32,
    }

    impl FnLower {
        fn new(def: &FnDef) -> FnLower {
            let mut slots = vec![0; def.locals as usize];
            // Parameters were declared first, so they hold local ids
            // `0..params` — and a call leaves the arguments in exactly those
            // slots (ADR-034).
            for i in 0..def.params {
                slots[i as usize] = i;
            }
            FnLower {
                code: Vec::new(),
                lines: Vec::new(),
                consts: Vec::new(),
                slots,
                next: def.params,
                regions: 0,
            }
        }

        /// The only place an instruction is created, which is what keeps `lines`
        /// parallel to `code` structurally rather than by discipline (ADR-023
        /// point 2). `../reg-lisp` lost that parallel in a mutant its whole
        /// suite failed to notice.
        fn emit(&mut self, i: Instr, o: SpanOrigin) -> Pc {
            self.code.push(i);
            self.lines.push(o);
            (self.code.len() - 1) as Pc
        }

        fn here(&self) -> Pc {
            self.code.len() as Pc
        }

        fn alloc(&mut self, n: u32) -> Slot {
            let s = self.next;
            self.next += n;
            s
        }

        fn patch(&mut self, at: Pc, target: Pc) {
            match &mut self.code[at as usize] {
                Instr::Jump { target: t }
                | Instr::JumpUnless { target: t, .. }
                | Instr::PushFinally { target: t }
                | Instr::PushHandler { catch: t, .. } => *t = target,
                other => unreachable!("cannot patch {other:?}"),
            }
        }

        /// Constants are deduplicated by language `=`, deliberately: it never
        /// merges `1` with `1.0` (Q13) or a list with a vector (Q20), and it
        /// never merges `##NaN` with itself, which costs a duplicate entry and
        /// is the correct answer under IEEE rules.
        fn konst(&mut self, v: &Value, dst: Slot, o: SpanOrigin) {
            let k = match self.consts.iter().position(|c| c == v) {
                Some(k) => k,
                None => {
                    self.consts.push(v.clone());
                    self.consts.len() - 1
                }
            };
            self.emit(
                Instr::Const {
                    dst,
                    k: k as ConstIdx,
                },
                o,
            );
        }

        /// An implicit `do`: every form but the last runs for effect, the last
        /// supplies the value. An empty body is `nil`.
        fn body(&mut self, lo: &mut Lower, exprs: &[Expr], dst: Slot, tail: bool, o: SpanOrigin) {
            match exprs.split_last() {
                None => self.konst(&Value::Nil, dst, o),
                Some((last, rest)) => {
                    for e in rest {
                        let scratch = self.alloc(1);
                        self.expr(lo, e, scratch, false);
                    }
                    self.expr(lo, last, dst, tail);
                }
            }
        }

        fn expr(&mut self, lo: &mut Lower, e: &Expr, dst: Slot, tail: bool) {
            let o = e.origin;
            match &e.core {
                Core::Literal(v) => self.konst(v, dst, o),
                Core::Local(id) => {
                    // No `src == dst` guard: `dst` is always an `alloc` result,
                    // `alloc` never returns a slot twice, and a local's slot is
                    // either a parameter index or a different `alloc` result —
                    // so the two can never coincide. A guard here would be dead
                    // code that reads as a peephole optimization, which is the
                    // shape the milestone-2 mutation pass already found once.
                    let src = self.slots[*id as usize];
                    self.emit(Instr::Move { dst, src }, o);
                }
                Core::Capture(i) => {
                    self.emit(Instr::GetCapture { dst, idx: *i }, o);
                }
                Core::SelfFn => {
                    self.emit(Instr::GetSelf { dst }, o);
                }
                Core::Global(s) => {
                    self.emit(Instr::GetGlobal { dst, name: *s }, o);
                }
                Core::If(test, then, other) => {
                    let cond = self.alloc(1);
                    self.expr(lo, test, cond, false);
                    let jump_else = self.emit(Instr::JumpUnless { cond, target: 0 }, o);
                    self.expr(lo, then, dst, tail);
                    let jump_end = self.emit(Instr::Jump { target: 0 }, o);
                    let els = self.here();
                    self.patch(jump_else, els);
                    self.expr(lo, other, dst, tail);
                    let end = self.here();
                    self.patch(jump_end, end);
                }
                Core::Do(es) => self.body(lo, es, dst, tail, o),
                Core::Let(binds, es) => {
                    for (id, init) in binds {
                        let s = self.alloc(1);
                        self.expr(lo, init, s, false);
                        self.slots[*id as usize] = s;
                    }
                    self.body(lo, es, dst, tail, o);
                }
                Core::Fn(def) => {
                    // Capture sources are named in the enclosing frame, so they
                    // are translated to slots here rather than inside the nested
                    // function (ADR-002: copied at creation, never referenced).
                    let captures = def
                        .captures
                        .iter()
                        .map(|c| match *c {
                            CaptureSpec::Local(id) => CaptureSrc::Local(self.slots[id as usize]),
                            CaptureSpec::Capture(i) => CaptureSrc::Capture(i),
                            CaptureSpec::SelfFn => CaptureSrc::SelfFn,
                        })
                        .collect();
                    let proto = lo.proto(def, captures);
                    self.emit(Instr::Closure { dst, proto }, o);
                }
                Core::Call(callee, args) => {
                    let argc = args.len() as u32;
                    // The whole window is reserved before anything is lowered
                    // into it, so a nested call inside an argument allocates
                    // above it rather than through it.
                    let base = self.alloc(1 + argc);
                    self.expr(lo, callee, base, false);
                    for (i, a) in args.iter().enumerate() {
                        self.expr(lo, a, base + 1 + i as u32, false);
                    }
                    if tail && self.regions == 0 {
                        self.emit(Instr::TailCall { base, argc }, o);
                    } else {
                        self.emit(Instr::Call { dst, base, argc }, o);
                    }
                }
                Core::SetCell(cell, val) => {
                    let c = self.alloc(1);
                    self.expr(lo, cell, c, false);
                    // Left to right (ADR-033), and the form's value is the value
                    // written — so it is lowered straight into `dst`.
                    self.expr(lo, val, dst, false);
                    self.emit(Instr::SetCell { cell: c, src: dst }, o);
                }
                Core::SetGlobal(name, val) => {
                    self.expr(lo, val, dst, false);
                    self.emit(
                        Instr::SetGlobal {
                            name: *name,
                            src: dst,
                        },
                        o,
                    );
                }
                Core::Throw(v) => {
                    let src = self.alloc(1);
                    self.expr(lo, v, src, false);
                    self.emit(Instr::Throw { src }, o);
                }
                Core::Try(t) => self.try_form(lo, t, dst, tail, o),
            }
        }

        /// Two nested handler regions, with `finally` emitted twice — once on
        /// the normal path, once as the path the VM enters while unwinding
        /// (ADR-034). Nesting the catch region *inside* the finally region is
        /// what makes a throw from the catch body still run the cleanup.
        ///
        /// The protocol milestone 4 has to honour: the VM pops a handler record
        /// when it dispatches to that record's target, and this code pops it
        /// with `POPHANDLER` on the path where nothing was thrown. Exactly one
        /// of those happens per record, which is ADR-028 invariant 1 read off
        /// the emitted code rather than argued about a state machine.
        fn try_form(&mut self, lo: &mut Lower, t: &TryForm, dst: Slot, tail: bool, o: SpanOrigin) {
            let fin = t.finally.as_ref().map(|_| {
                self.regions += 1;
                self.emit(Instr::PushFinally { target: 0 }, o)
            });
            let cat = t.catch.as_ref().map(|(id, _)| {
                let err = self.alloc(1);
                self.slots[*id as usize] = err;
                self.regions += 1;
                self.emit(Instr::PushHandler { catch: 0, err }, o)
            });

            // `tail` passes through untouched. Whether a call in the protected
            // body is *actually* a tail call is the `regions` counter's
            // business, decided where the call is emitted — one mechanism
            // rather than two that have to agree. Clearing the flag here as
            // well read as belt-and-braces and was really a way for the counter
            // to be dead and untestable: a mutation that deleted it left the
            // whole suite green (`docs/notes/milestone-2-mutants.md`).
            self.body(lo, &t.body, dst, tail, o);

            if let (Some(push), Some((_, handler))) = (cat, t.catch.as_ref()) {
                self.emit(Instr::PopHandler, o);
                self.regions -= 1;
                let over = self.emit(Instr::Jump { target: 0 }, o);
                let entry = self.here();
                self.patch(push, entry);
                self.body(lo, handler, dst, tail, o);
                let done = self.here();
                self.patch(over, done);
            }

            if let (Some(push), Some(cleanup)) = (fin, t.finally.as_ref()) {
                self.emit(Instr::PopHandler, o);
                self.regions -= 1;
                // The cleanup's value is discarded on both paths, so it is never
                // in tail position and wants a slot only to be written to.
                let normal = self.alloc(1);
                self.body(lo, cleanup, normal, false, o);
                let over = self.emit(Instr::Jump { target: 0 }, o);
                let entry = self.here();
                self.patch(push, entry);
                let unwinding = self.alloc(1);
                self.body(lo, cleanup, unwinding, false, o);
                self.emit(Instr::EndFinally, o);
                let end = self.here();
                self.patch(over, end);
            }
        }
    }
}

// ---------------------------------------------------------------------------

/// The VM: frames, calls, closures, tail calls, and the handler stack
/// (ADR-004, ADR-038, ADR-039).
///
/// The dispatch loop is flat and the Rust stack is empty at every instruction
/// boundary (ADR-004). That is the decision the whole snapshot story rests on,
/// and it is also why nothing here recurses — a native function is called and
/// returns, but it never re-enters the loop.
///
/// `Vm` and `Execution` are separate for the reason ADR-029 gives: an `Image` is
/// one of each, so anything not in either is out of scope by construction.
/// Milestones 3 and 4 build both; fuel is milestone 8.
pub mod vm {
    use crate::bytecode::{CaptureSrc, Chunk, Instr, Slot};
    use crate::error::SpanOrigin;
    use crate::printer;
    use crate::value::{
        CellId, Closure, Interner, KwId, ListObj, MapObj, NativeId, StrObj, SymId, Value, VecObj,
    };
    use std::rc::Rc;

    /// How a run ended. Both endings are language values (ADR-039): a fault the
    /// VM raises unwinds exactly as `throw` does, so there is one failure path
    /// and `Threw` is all of it.
    #[derive(Debug)]
    pub enum Outcome {
        Returned(Value),
        Threw(Unwind),
    }

    /// A failure in flight. The value is what a `catch` binds; the origin and
    /// the suppressed chain travel *beside* it rather than inside it (ADR-039
    /// clause 4). That is ADR-026's rule for origins, and it is the only shape
    /// that still works when the thrown value is an integer.
    #[derive(Debug)]
    pub struct Unwind {
        pub value: Value,
        /// The instruction that raised it — the only place the position is
        /// known, because a value carries none.
        pub origin: SpanOrigin,
        /// Errors this one displaced, newest first (ADR-028 invariant 3).
        pub suppressed: Vec<Value>,
    }

    impl Unwind {
        pub fn new(value: Value, origin: SpanOrigin) -> Unwind {
            Unwind {
                value,
                origin,
                suppressed: Vec::new(),
            }
        }

        /// `path:line:col` for the instruction that raised it, where known.
        pub fn position(&self, path: &str, src: &str) -> Option<String> {
            let s = self.origin.span()?;
            let (line, col) = crate::error::line_col(src, s.start as usize);
            Some(format!("{path}:{line}:{col}"))
        }

        /// Take over an error this one displaced, chain and all.
        fn suppress(&mut self, other: Unwind) {
            self.suppressed.push(other.value);
            self.suppressed.extend(other.suppressed);
        }
    }

    /// The closed `:kind` vocabulary for `:type :vm-error` (ADR-039 clause 3).
    /// Adding one is an ADR, not a new keyword at a raise site — an open set of
    /// keywords is a formatted string wearing a colon.
    #[derive(Clone, Copy, Debug)]
    pub enum Kind {
        Arity,
        Unbound,
        NotCallable,
        Type,
        Overflow,
        /// A decision this language has deliberately not taken, reached at run
        /// time. Q26's floats are the only one so far.
        Undecided,
        /// A VM invariant no program should be able to reach.
        Internal,
    }

    impl Kind {
        /// In discriminant order, which is what `kind as usize` indexes.
        const ALL: [Kind; 7] = [
            Kind::Arity,
            Kind::Unbound,
            Kind::NotCallable,
            Kind::Type,
            Kind::Overflow,
            Kind::Undecided,
            Kind::Internal,
        ];

        fn name(self) -> &'static str {
            match self {
                Kind::Arity => "arity",
                Kind::Unbound => "unbound",
                Kind::NotCallable => "not-callable",
                Kind::Type => "type",
                Kind::Overflow => "overflow",
                Kind::Undecided => "undecided",
                Kind::Internal => "internal",
            }
        }
    }

    /// A fault before it has a position: what went wrong, and how to say it to
    /// a human. Natives raise these; the dispatch loop turns one into an
    /// `Unwind` at the instruction that raised it.
    #[derive(Debug)]
    pub struct Fault {
        pub kind: Kind,
        pub msg: String,
    }

    pub fn fault(kind: Kind, msg: impl Into<String>) -> Fault {
        Fault {
            kind,
            msg: msg.into(),
        }
    }

    type NativeFn = fn(&mut Vm, &[Value]) -> Result<Value, Fault>;

    struct Native {
        name: SymId,
        /// Minimum argument count; `variadic` allows more.
        min: u32,
        variadic: bool,
        f: NativeFn,
    }

    /// ADR-025: cells are retained for the lifetime of the VM in v1, and the
    /// live count is instrumented rather than reclaimed.
    struct CellEntry {
        generation: u32,
        value: Value,
    }

    /// The keywords a fault value is built from, interned once at construction
    /// (ADR-039 clause 3). The closed vocabulary is readable here, in one
    /// place, instead of being spread over the raise sites.
    struct Kws {
        type_: KwId,
        kind: KwId,
        message: KwId,
        vm_error: KwId,
        /// Indexed by `Kind as usize`.
        kinds: Vec<KwId>,
    }

    pub struct Vm {
        pub interner: Interner,
        /// Indexed by `SymId`, not a map: globals reach output through the
        /// disassembler and eventually through an `Image`, and a `Vec` is
        /// deterministic by construction where a `HashMap` needs a sort
        /// (BUILD.md, determinism).
        globals: Vec<Option<CellId>>,
        cells: Vec<CellEntry>,
        natives: Vec<Native>,
        kws: Kws,
        /// The buffered in-memory host BUILD.md's serialization property needs:
        /// emitted effects are part of the comparison rather than escaping it.
        out: String,
    }

    impl Vm {
        pub fn new() -> Vm {
            let mut interner = Interner::new();
            let kws = Kws {
                type_: KwId(interner.intern("type")),
                kind: KwId(interner.intern("kind")),
                message: KwId(interner.intern("message")),
                vm_error: KwId(interner.intern("vm-error")),
                kinds: Kind::ALL
                    .iter()
                    .map(|k| KwId(interner.intern(k.name())))
                    .collect(),
            };
            debug_assert!(
                Kind::ALL.iter().enumerate().all(|(i, k)| *k as usize == i),
                "Kind::ALL is not in discriminant order, so `kind as usize` indexes the wrong keyword"
            );
            let mut vm = Vm {
                interner,
                globals: Vec::new(),
                cells: Vec::new(),
                natives: Vec::new(),
                kws,
                out: String::new(),
            };
            vm.install_primitives();
            vm
        }

        /// ADR-039 clause 3: a VM-raised fault is a language value of exactly
        /// this shape. `:kind` is the contract; `:message` is prose.
        fn fault_value(&self, f: &Fault) -> Value {
            let kw = Value::Keyword;
            Value::Map(Rc::new(MapObj(vec![
                (kw(self.kws.type_), kw(self.kws.vm_error)),
                (kw(self.kws.kind), kw(self.kws.kinds[f.kind as usize])),
                (
                    kw(self.kws.message),
                    Value::Str(Rc::new(StrObj(f.msg.clone()))),
                ),
            ])))
        }
    }

    /// Give a fault its position and make it an unwind. Every VM-raised failure
    /// goes through here, which is what makes "a fault is a throw" one line
    /// rather than a claim (ADR-039 clause 2).
    fn raise(vm: &Vm, kind: Kind, at: SpanOrigin, msg: impl Into<String>) -> Unwind {
        Unwind::new(vm.fault_value(&fault(kind, msg)), at)
    }

    impl Vm {
        pub fn take_output(&mut self) -> String {
            std::mem::take(&mut self.out)
        }

        pub fn live_cells(&self) -> usize {
            self.cells.len()
        }

        fn new_cell(&mut self, value: Value) -> CellId {
            self.cells.push(CellEntry {
                generation: 0,
                value,
            });
            CellId((self.cells.len() - 1) as u32, 0)
        }

        fn cell(&self, id: CellId) -> Option<&Value> {
            let e = self.cells.get(id.0 as usize)?;
            (e.generation == id.1).then_some(&e.value)
        }

        fn set_cell(&mut self, id: CellId, value: Value) -> bool {
            match self.cells.get_mut(id.0 as usize) {
                Some(e) if e.generation == id.1 => {
                    e.value = value;
                    true
                }
                _ => false,
            }
        }

        pub fn global(&self, name: SymId) -> Option<&Value> {
            let id = (*self.globals.get(name.0 as usize)?)?;
            self.cell(id)
        }

        /// ADR-027's create-or-rebind. The name's cell is created once and kept,
        /// so a closure that already read the global keeps seeing rebinds.
        pub fn set_global(&mut self, name: SymId, value: Value) {
            let i = name.0 as usize;
            if self.globals.len() <= i {
                self.globals.resize(i + 1, None);
            }
            match self.globals[i] {
                Some(id) => {
                    self.set_cell(id, value);
                }
                None => {
                    let id = self.new_cell(value);
                    self.globals[i] = Some(id);
                }
            }
        }

        fn native(&mut self, name: &str, min: u32, variadic: bool, f: NativeFn) {
            let sym = SymId(self.interner.intern(name));
            self.natives.push(Native {
                name: sym,
                min,
                variadic,
                f,
            });
            let id = NativeId((self.natives.len() - 1) as u32);
            self.set_global(sym, Value::Fn(Rc::new(Closure::Native(id))));
        }
    }

    impl Default for Vm {
        fn default() -> Vm {
            Vm::new()
        }
    }

    /// One function activation. Slots live in a single flat `Vec` on the
    /// `Execution`, base-relative, which is the shape an `Image` wants
    /// (ADR-029) and what makes a tail call a truncation rather than a copy.
    struct Frame {
        proto: u32,
        pc: usize,
        /// Index in `Execution::slots` of this frame's slot 0.
        base: usize,
        /// Absolute slot index, in the *caller's* frame, for the return value.
        /// Always below `base`, so returning can truncate first and write after.
        dst: usize,
        /// The slot stack's length before this frame existed. Returning
        /// restores it. Truncating to `base` instead would discard the part of
        /// the *caller's* frame that sits above the call window, which is most
        /// of it — the window is allocated early and the caller keeps using
        /// slots above it after the call returns.
        ret_len: usize,
        closure: Rc<Closure>,
    }

    /// One active `try` region (ADR-028). `err` is the whole difference between
    /// the two kinds: a catch has a slot to bind the thrown value to, a finally
    /// has nothing to bind.
    struct Handler {
        /// The frame that owns the record. Unwinding to it drops every frame
        /// above, which is what makes a handler survive a call.
        frame: usize,
        target: usize,
        err: Option<Slot>,
    }

    /// An unwind parked while a cleanup runs. `depth` is the handler depth the
    /// cleanup body runs at: an unwind that escapes *below* it displaces this
    /// one (ADR-028 invariant 3), and one that does not is a throw the cleanup
    /// caught itself.
    struct Pending {
        depth: usize,
        unwind: Unwind,
    }

    pub struct Execution {
        frames: Vec<Frame>,
        slots: Vec<Value>,
        /// ADR-028: active handlers and finalizers live in VM-owned memory,
        /// reachable from the image — never on the Rust stack.
        handlers: Vec<Handler>,
        pending: Vec<Pending>,
    }

    impl Execution {
        /// The deepest the frame stack reached, and the largest the slot stack
        /// grew. A tail loop keeps both flat, which is milestone 3's exit
        /// condition and not otherwise observable from outside.
        pub fn depth(&self) -> usize {
            self.frames.len()
        }

        pub fn slot_count(&self) -> usize {
            self.slots.len()
        }
    }

    pub fn run(vm: &mut Vm, chunk: &Chunk) -> Outcome {
        run_traced(vm, chunk).0
    }

    /// `run`, plus the high-water marks. Separate because the marks exist for
    /// the constant-space property and have no place in the driver.
    pub fn run_traced(vm: &mut Vm, chunk: &Chunk) -> (Outcome, (usize, usize)) {
        let top = Rc::new(Closure::Fn {
            proto: 0,
            captures: Rc::from(Vec::new()),
        });
        let mut ex = Execution {
            frames: Vec::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            pending: Vec::new(),
        };
        let slots = chunk.protos[0].slots as usize;
        ex.slots.resize(slots, Value::Nil);
        ex.frames.push(Frame {
            proto: 0,
            pc: 0,
            base: 0,
            dst: 0,
            ret_len: 0,
            closure: top,
        });

        let mut peak = (1usize, ex.slots.len());
        loop {
            let fi = ex.frames.len() - 1;
            let (pidx, pc) = {
                let f = &ex.frames[fi];
                (f.proto, f.pc)
            };
            let proto = &chunk.protos[pidx as usize];
            let ins = proto.code[pc];
            let at = proto.lines[pc];
            ex.frames[fi].pc = pc + 1;

            match exec(vm, &mut ex, chunk, ins, at) {
                Ok(Step::Next) => {}
                Ok(Step::Done(v)) => {
                    // A record left open by a return is a compiler bug, not a
                    // program error (ADR-028 rule 5). Checked rather than
                    // asserted in a comment: dropping the `POPHANDLER` on the
                    // untroubled path is the first mutation anyone would try.
                    assert!(
                        ex.handlers.is_empty() && ex.pending.is_empty(),
                        "the run finished with {} handler(s) and {} parked unwind(s) open",
                        ex.handlers.len(),
                        ex.pending.len()
                    );
                    return (Outcome::Returned(v), peak);
                }
                // ADR-039: one failure path. A fault and a `throw` arrive here
                // identically, and unwinding is what runs the cleanups.
                Err(u) => {
                    if let Some(escaped) = unwind(&mut ex, u) {
                        return (Outcome::Threw(escaped), peak);
                    }
                }
            }
            peak.0 = peak.0.max(ex.frames.len());
            peak.1 = peak.1.max(ex.slots.len());
        }
    }

    /// What executing one instruction did: carried on, or finished the run by
    /// returning from the outermost frame.
    enum Step {
        Next,
        Done(Value),
    }

    /// One instruction. Split out of the loop so the fallible arms can use `?`
    /// and so an unwind is born in exactly one place.
    fn exec(
        vm: &mut Vm,
        ex: &mut Execution,
        chunk: &Chunk,
        ins: Instr,
        at: SpanOrigin,
    ) -> Result<Step, Unwind> {
        let fi = ex.frames.len() - 1;
        let base = ex.frames[fi].base;
        let proto = &chunk.protos[ex.frames[fi].proto as usize];
        match ins {
            Instr::Const { dst, k } => {
                ex.slots[base + dst as usize] = proto.consts[k as usize].clone();
            }
            Instr::Move { dst, src } => {
                ex.slots[base + dst as usize] = ex.slots[base + src as usize].clone();
            }
            Instr::GetGlobal { dst, name } => {
                let v = vm.global(name).cloned().ok_or_else(|| {
                    raise(
                        vm,
                        Kind::Unbound,
                        at,
                        format!("`{}` is not bound", vm.interner.name(name.0)),
                    )
                })?;
                ex.slots[base + dst as usize] = v;
            }
            Instr::SetGlobal { name, src } => {
                let v = ex.slots[base + src as usize].clone();
                vm.set_global(name, v);
            }
            Instr::GetCapture { dst, idx } => {
                let v = match &*ex.frames[fi].closure {
                    Closure::Fn { captures, .. } => captures[idx as usize].clone(),
                    Closure::Native(_) => {
                        return Err(raise(
                            vm,
                            Kind::Internal,
                            at,
                            "a native function has no captures",
                        ))
                    }
                };
                ex.slots[base + dst as usize] = v;
            }
            Instr::GetSelf { dst } => {
                let me = ex.frames[fi].closure.clone();
                ex.slots[base + dst as usize] = Value::Fn(me);
            }
            Instr::SetCell { cell, src } => {
                let target = ex.slots[base + cell as usize].clone();
                let v = ex.slots[base + src as usize].clone();
                match target {
                    Value::Cell(id) if vm.set_cell(id, v) => {}
                    Value::Cell(_) => {
                        return Err(raise(vm, Kind::Internal, at, "cell is no longer live"))
                    }
                    other => {
                        return Err(raise(
                            vm,
                            Kind::Type,
                            at,
                            format!(
                                "`set-cell!` needs a cell, not a {}",
                                crate::value::kind_name(&other)
                            ),
                        ))
                    }
                }
            }
            Instr::Closure { dst, proto: p } => {
                // ADR-002: captures are copied out of this frame now, not
                // referenced. The descriptors are read here because they
                // name slots in the *enclosing* frame.
                let target = &chunk.protos[p as usize];
                let mut captured = Vec::with_capacity(target.captures.len());
                for c in &target.captures {
                    captured.push(match *c {
                        CaptureSrc::Local(s) => ex.slots[base + s as usize].clone(),
                        CaptureSrc::Capture(i) => match &*ex.frames[fi].closure {
                            Closure::Fn { captures, .. } => captures[i as usize].clone(),
                            Closure::Native(_) => {
                                return Err(raise(
                                    vm,
                                    Kind::Internal,
                                    at,
                                    "a native function has no captures",
                                ))
                            }
                        },
                        CaptureSrc::SelfFn => Value::Fn(ex.frames[fi].closure.clone()),
                    });
                }
                ex.slots[base + dst as usize] = Value::Fn(Rc::new(Closure::Fn {
                    proto: p,
                    captures: Rc::from(captured),
                }));
            }
            Instr::Jump { target } => {
                ex.frames[fi].pc = target as usize;
            }
            Instr::JumpUnless { cond, target } => {
                // Only nil and false are falsy (`TRAPS.md`). 0 and "" are
                // truthy, and so is an empty collection.
                let truthy = !matches!(
                    ex.slots[base + cond as usize],
                    Value::Nil | Value::Bool(false)
                );
                if !truthy {
                    ex.frames[fi].pc = target as usize;
                }
            }
            Instr::Call {
                dst,
                base: cb,
                argc,
            } => {
                let callee = base + cb as usize;
                if let Some(v) = call(vm, ex, chunk, callee, argc, base + dst as usize, at)? {
                    // A native returns immediately; there is no frame to
                    // push and nothing to resume.
                    ex.slots[base + dst as usize] = v;
                }
            }
            Instr::TailCall { base: cb, argc } => {
                let callee = base + cb as usize;
                if let Some(v) = tail_call(vm, ex, chunk, callee, argc, at)? {
                    if let Some(done) = ret(ex, v) {
                        return Ok(Step::Done(done));
                    }
                }
            }
            Instr::Return { src } => {
                let v = ex.slots[base + src as usize].clone();
                if let Some(done) = ret(ex, v) {
                    return Ok(Step::Done(done));
                }
            }
            Instr::Throw { src } => {
                return Err(Unwind::new(ex.slots[base + src as usize].clone(), at));
            }
            // The four handler instructions. A record is pushed here, and
            // removed either by the `POPHANDLER` on the path where nothing
            // was thrown or by `unwind` dispatching to it — never both,
            // which is ADR-028 invariant 1 read off the bytecode.
            Instr::PushHandler { catch, err } => ex.handlers.push(Handler {
                frame: fi,
                target: catch as usize,
                err: Some(err),
            }),
            Instr::PushFinally { target } => ex.handlers.push(Handler {
                frame: fi,
                target: target as usize,
                err: None,
            }),
            Instr::PopHandler => {
                let h = ex
                    .handlers
                    .pop()
                    .expect("POPHANDLER with no open handler region");
                debug_assert_eq!(h.frame, fi, "a handler record outlived its frame");
            }
            Instr::EndFinally => {
                // Only the unwinding copy of a cleanup ends here (ADR-034),
                // so there is always something parked: pick it back up and
                // carry on unwinding.
                let p = ex
                    .pending
                    .pop()
                    .expect("ENDFINALLY outside an unwinding cleanup");
                return Err(p.unwind);
            }
        }
        Ok(Step::Next)
    }

    /// Deliver an unwind to the innermost handler record, or hand it back when
    /// the handler stack is empty and the run is over.
    ///
    /// Exactly one record is popped, because every record is *entered* when
    /// unwinding reaches it: a catch binds the value, a finally parks it and
    /// runs the cleanup. Nothing here searches for a matching handler — there is
    /// no filter to match on (ADR-039 clause 5).
    fn unwind(ex: &mut Execution, mut u: Unwind) -> Option<Unwind> {
        let h = match ex.handlers.pop() {
            Some(h) => h,
            None => {
                // Nothing left to catch it, so every parked error it displaced
                // is retained on it (ADR-028 invariant 3).
                while let Some(p) = ex.pending.pop() {
                    u.suppress(p.unwind);
                }
                return Some(u);
            }
        };
        // A parked unwind whose cleanup this one has escaped is displaced by it.
        // One that is caught *inside* the cleanup never unwinds below the depth
        // that cleanup runs at, so it leaves the parked error alone.
        while ex
            .pending
            .last()
            .is_some_and(|p| p.depth > ex.handlers.len())
        {
            let p = ex.pending.pop().expect("just checked");
            u.suppress(p.unwind);
        }
        // Frames above the record's owner are gone, through the same call a
        // normal return uses — see `drop_frame` for why that matters.
        while ex.frames.len() > h.frame + 1 {
            drop_frame(ex);
        }
        let base = ex.frames[h.frame].base;
        ex.frames[h.frame].pc = h.target;
        match h.err {
            // The handler binds the value alone. Position and the suppressed
            // chain end here, by decision (ADR-039 clause 4).
            Some(slot) => ex.slots[base + slot as usize] = u.value,
            None => ex.pending.push(Pending {
                depth: ex.handlers.len(),
                unwind: u,
            }),
        }
        None
    }

    /// Pop one frame and give its slots back.
    ///
    /// The only place a frame is released, so returning and unwinding cannot
    /// disagree about what that means — and the milestone-4 mutation pass says
    /// that sharing is the *only* thing keeping the unwinding side honest.
    /// Dropping the frames while keeping their slots leaves every test green:
    /// the leak is bounded, never reaches a value, and cannot move a high-water
    /// mark. It is visible only as dead slots carried into later frames and
    /// into an `Image` (ADR-029, `notes/milestone-4-mutants.md`).
    fn drop_frame(ex: &mut Execution) -> Frame {
        let f = ex.frames.pop().expect("a frame to drop");
        ex.slots.truncate(f.ret_len);
        f
    }

    /// Pop the finished frame and deliver its value. `None` means execution
    /// continues; `Some` means the outermost frame returned.
    fn ret(ex: &mut Execution, value: Value) -> Option<Value> {
        let f = drop_frame(ex);
        if ex.frames.is_empty() {
            return Some(value);
        }
        ex.slots[f.dst] = value;
        None
    }

    /// Push a frame for a bytecode callee, or run a native and hand back its
    /// value. `Ok(None)` means a frame was pushed.
    fn call(
        vm: &mut Vm,
        ex: &mut Execution,
        chunk: &Chunk,
        callee: usize,
        argc: u32,
        dst: usize,
        at: SpanOrigin,
    ) -> Result<Option<Value>, Unwind> {
        match callee_at(vm, ex, callee, at)? {
            Callee::Native(id) => native_call(vm, ex, id, callee, argc, at).map(Some),
            Callee::Bytecode(proto, c) => {
                // The arguments already sit at `callee+1..`, which is exactly
                // where the callee's slots 0.. must be — so a call copies
                // nothing (ADR-034).
                let base = callee + 1;
                let ret_len = ex.slots.len();
                enter(vm, ex, chunk, proto, argc, base, at)?;
                ex.frames.push(Frame {
                    proto,
                    pc: 0,
                    base,
                    dst,
                    ret_len,
                    closure: c,
                });
                Ok(None)
            }
        }
    }

    /// ADR-028: reuse the caller's frame. The arguments move down to `base`,
    /// which is what keeps a tail loop in constant space.
    fn tail_call(
        vm: &mut Vm,
        ex: &mut Execution,
        chunk: &Chunk,
        callee: usize,
        argc: u32,
        at: SpanOrigin,
    ) -> Result<Option<Value>, Unwind> {
        match callee_at(vm, ex, callee, at)? {
            Callee::Native(id) => native_call(vm, ex, id, callee, argc, at).map(Some),
            Callee::Bytecode(proto, c) => {
                let fi = ex.frames.len() - 1;
                let base = ex.frames[fi].base;
                // Copying ascending is safe: `base <= callee`, so the
                // destination of each argument is always below its source.
                for i in 0..argc as usize {
                    ex.slots[base + i] = ex.slots[callee + 1 + i].clone();
                }
                enter(vm, ex, chunk, proto, argc, base, at)?;
                ex.frames[fi].proto = proto;
                ex.frames[fi].pc = 0;
                ex.frames[fi].closure = c;
                Ok(None)
            }
        }
    }

    /// What is in the callee slot, resolved once so `call` and `tail_call`
    /// share one answer and one error message.
    enum Callee {
        Native(NativeId),
        Bytecode(u32, Rc<Closure>),
    }

    fn callee_at(vm: &Vm, ex: &Execution, callee: usize, at: SpanOrigin) -> Result<Callee, Unwind> {
        match &ex.slots[callee] {
            Value::Fn(c) => Ok(match &**c {
                Closure::Native(id) => Callee::Native(*id),
                Closure::Fn { proto, .. } => Callee::Bytecode(*proto, c.clone()),
            }),
            other => Err(raise(
                vm,
                Kind::NotCallable,
                at,
                format!("cannot call a {}", crate::value::kind_name(other)),
            )),
        }
    }

    /// The callee prologue: check arity once, at call time (ADR-033 rule 2),
    /// size the frame, and pack a rest parameter (ADR-038).
    fn enter(
        vm: &Vm,
        ex: &mut Execution,
        chunk: &Chunk,
        proto: u32,
        argc: u32,
        base: usize,
        at: SpanOrigin,
    ) -> Result<(), Unwind> {
        let p = &chunk.protos[proto as usize];
        let name = match p.name {
            Some(s) => vm.interner.name(s.0).to_string(),
            None => "fn".to_string(),
        };
        let fixed = if p.variadic { p.params - 1 } else { p.params };
        if argc < fixed || (!p.variadic && argc > fixed) {
            let wanted = if p.variadic {
                format!("at least {fixed}")
            } else {
                fixed.to_string()
            };
            return Err(raise(
                vm,
                Kind::Arity,
                at,
                format!("`{name}` takes {wanted} argument(s), given {argc}"),
            ));
        }
        // ADR-038: whatever the call actually needs, since ADR-034 sets no
        // maximum arity and the compiler therefore cannot have reserved it.
        let want = (p.slots as usize).max(argc as usize);
        if ex.slots.len() < base + want {
            ex.slots.resize(base + want, Value::Nil);
        }
        // Everything above the arguments starts as nil. The compiler writes
        // every slot before it reads one, so this is not needed for
        // correctness — it is needed so a frame's contents are a function of
        // the call and not of whatever ran here last, which is what an `Image`
        // has to serialize (ADR-029).
        for s in ex
            .slots
            .iter_mut()
            .skip(base + argc as usize)
            .take(want.saturating_sub(argc as usize))
        {
            *s = Value::Nil;
        }
        if p.variadic {
            // Packed in place: slots `fixed..argc` are dead the instant they are
            // collected, because the prologue runs before the first
            // instruction. The rest is an empty *list*, never nil (ADR-033,
            // E-11).
            let rest: Vec<Value> = ex.slots[base + fixed as usize..base + argc as usize].to_vec();
            for s in ex
                .slots
                .iter_mut()
                .skip(base + fixed as usize)
                .take(want - fixed as usize)
            {
                *s = Value::Nil;
            }
            ex.slots[base + fixed as usize] = Value::List(Rc::new(ListObj(rest)));
        }
        Ok(())
    }

    fn native_call(
        vm: &mut Vm,
        ex: &Execution,
        id: NativeId,
        callee: usize,
        argc: u32,
        at: SpanOrigin,
    ) -> Result<Value, Unwind> {
        let n = &vm.natives[id.0 as usize];
        let (min, variadic, f, name) = (n.min, n.variadic, n.f, n.name);
        if argc < min || (!variadic && argc > min) {
            let wanted = if variadic {
                format!("at least {min}")
            } else {
                min.to_string()
            };
            return Err(raise(
                vm,
                Kind::Arity,
                at,
                format!(
                    "`{}` takes {wanted} argument(s), given {argc}",
                    vm.interner.name(name.0)
                ),
            ));
        }
        // `ex` and `vm` are distinct, so the arguments are borrowed in place
        // rather than copied into a temporary for every native call.
        let args = &ex.slots[callee + 1..callee + 1 + argc as usize];
        // A native raises a `Fault`, which has no position: it acquires the
        // calling instruction's origin here, the same one a bytecode callee's
        // arity error gets.
        f(vm, args).map_err(|e| Unwind::new(vm.fault_value(&e), at))
    }

    // --- primitives ---------------------------------------------------------

    /// ADR-038: primitives are ordinary globals, so `+` is a value you can pass
    /// to `map` and `set-global!` can rebind it — including out from under the
    /// program, which is the wart that entry accepts.
    impl Vm {
        fn install_primitives(&mut self) {
            self.native("+", 0, true, |_, a| int_fold(a, 0, i64::checked_add, "+"));
            self.native("*", 0, true, |_, a| int_fold(a, 1, i64::checked_mul, "*"));
            self.native("-", 1, true, |_, a| {
                let first = int_arg(&a[0], "-")?;
                if a.len() == 1 {
                    return first
                        .checked_neg()
                        .map(Value::Int)
                        .ok_or_else(|| overflow("-"));
                }
                let mut acc = first;
                for v in &a[1..] {
                    acc = acc
                        .checked_sub(int_arg(v, "-")?)
                        .ok_or_else(|| overflow("-"))?;
                }
                Ok(Value::Int(acc))
            });
            self.native("<", 2, false, |_, a| {
                Ok(Value::Bool(int_arg(&a[0], "<")? < int_arg(&a[1], "<")?))
            });
            self.native(">", 2, false, |_, a| {
                Ok(Value::Bool(int_arg(&a[0], ">")? > int_arg(&a[1], ">")?))
            });
            self.native("println", 0, true, |vm, a| {
                let line: Vec<String> = a
                    .iter()
                    .map(|v| printer::display(v, &vm.interner))
                    .collect();
                vm.out.push_str(&line.join(" "));
                vm.out.push('\n');
                Ok(Value::Nil)
            });
            self.native("list", 0, true, |_, a| {
                Ok(Value::List(Rc::new(ListObj(a.to_vec()))))
            });
            // ADR-035 lowers `[a b]` to a call, so these two are what makes a
            // collection literal mean anything. The representation itself is
            // still Q6's, and `hash-map` keeps duplicate keys because Q20 has
            // not said what to do with them.
            self.native("vector", 0, true, |_, a| {
                Ok(Value::Vec(Rc::new(VecObj(a.to_vec()))))
            });
            self.native("hash-map", 0, true, |_, a| {
                if !a.len().is_multiple_of(2) {
                    return Err(fault(Kind::Arity, "`hash-map` needs a value for every key"));
                }
                Ok(Value::Map(Rc::new(MapObj(
                    a.chunks(2).map(|p| (p[0].clone(), p[1].clone())).collect(),
                ))))
            });
        }
    }

    fn overflow(op: &str) -> Fault {
        fault(
            Kind::Overflow,
            format!("`{op}` overflowed a 64-bit integer (ADR-037)"),
        )
    }

    /// Q26: mixing integers and floats, and float arithmetic at all, is
    /// undecided. Faulting is the option that does not answer it by accident —
    /// silently coercing would fix the numeric tower here, in a match arm.
    fn int_arg(v: &Value, op: &str) -> Result<i64, Fault> {
        match v {
            Value::Int(i) => Ok(*i),
            Value::Float(_) => Err(fault(
                Kind::Undecided,
                format!("`{op}` on a float: the numeric tower is undecided (Q26)"),
            )),
            other => Err(fault(
                Kind::Type,
                format!(
                    "`{op}` needs an integer, not a {}",
                    crate::value::kind_name(other)
                ),
            )),
        }
    }

    fn int_fold(
        args: &[Value],
        init: i64,
        op: fn(i64, i64) -> Option<i64>,
        name: &str,
    ) -> Result<Value, Fault> {
        let mut acc = init;
        for v in args {
            acc = op(acc, int_arg(v, name)?).ok_or_else(|| overflow(name))?;
        }
        Ok(Value::Int(acc))
    }
}
